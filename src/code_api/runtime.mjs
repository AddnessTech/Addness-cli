import { spawn } from "node:child_process";

const DEFAULT_TIMEOUT_MS = 120_000;
const DEFAULT_MAX_OUTPUT_BYTES = 64 * 1024 * 1024;

export class AddnessCodeApiError extends Error {
  constructor(message, details = {}) {
    super(message);
    this.name = "AddnessCodeApiError";
    this.details = details;
  }
}

function isMissing(value) {
  return value === undefined || value === null;
}

function encodedValue(parameter, value) {
  if (parameter.jsonValue && typeof value !== "string") {
    try {
      const encoded = JSON.stringify(value);
      if (typeof encoded !== "string") {
        throw new TypeError("value has no JSON representation");
      }
      return encoded;
    } catch (error) {
      throw new AddnessCodeApiError(
        `${parameter.name} could not be serialized as JSON: ${error.message}`,
      );
    }
  }
  if (!["string", "number", "boolean"].includes(typeof value)) {
    throw new AddnessCodeApiError(
      `${parameter.name} must be a string, number, or boolean`,
    );
  }
  if (typeof value === "number" && !Number.isFinite(value)) {
    throw new AddnessCodeApiError(`${parameter.name} must be a finite number`);
  }
  return String(value);
}

function parameterFlag(parameter) {
  if (typeof parameter.flag !== "string" || !parameter.flag.startsWith("-")) {
    throw new AddnessCodeApiError(`Invalid flag definition: ${parameter.name}`);
  }
  return parameter.flag;
}

function appendParameter(args, parameter, value) {
  if (isMissing(value)) {
    if (parameter.required) {
      throw new AddnessCodeApiError(`Missing required parameter: ${parameter.name}`);
    }
    return;
  }

  switch (parameter.kind) {
    case "positional":
      args.push(encodedValue(parameter, value));
      return;
    case "positionalMany": {
      if (!Array.isArray(value)) {
        throw new AddnessCodeApiError(`${parameter.name} must be an array`);
      }
      if (parameter.required && value.length === 0) {
        throw new AddnessCodeApiError(`${parameter.name} must not be empty`);
      }
      args.push(...value.map((item) => encodedValue(parameter, item)));
      return;
    }
    case "flag":
      if (typeof value !== "boolean") {
        throw new AddnessCodeApiError(`${parameter.name} must be a boolean`);
      }
      if (value) args.push(parameterFlag(parameter));
      return;
    case "negatedFlag":
      if (typeof value !== "boolean") {
        throw new AddnessCodeApiError(`${parameter.name} must be a boolean`);
      }
      if (!value) args.push(parameterFlag(parameter));
      return;
    case "count": {
      if (!Number.isSafeInteger(value) || value < 0) {
        throw new AddnessCodeApiError(`${parameter.name} must be a non-negative integer`);
      }
      for (let index = 0; index < value; index += 1) {
        args.push(parameterFlag(parameter));
      }
      return;
    }
    case "append": {
      if (!Array.isArray(value)) {
        throw new AddnessCodeApiError(`${parameter.name} must be an array`);
      }
      for (const item of value) {
        args.push(parameterFlag(parameter), encodedValue(parameter, item));
      }
      return;
    }
    case "option":
      args.push(parameterFlag(parameter), encodedValue(parameter, value));
      return;
    default:
      throw new AddnessCodeApiError(`Unsupported parameter kind: ${parameter.kind}`);
  }
}

function buildArguments(definition, input) {
  if (
    definition === null ||
    typeof definition !== "object" ||
    !Array.isArray(definition.command) ||
    definition.command.length === 0 ||
    !definition.command.every((part) => typeof part === "string" && part.length > 0) ||
    !Array.isArray(definition.parameters)
  ) {
    throw new AddnessCodeApiError("Invalid operation definition");
  }
  if (input === null || Array.isArray(input) || typeof input !== "object") {
    throw new AddnessCodeApiError("input must be an object");
  }
  const known = new Set(definition.parameters.map((parameter) => parameter.name));
  const unknown = Object.keys(input).filter((name) => !known.has(name));
  if (unknown.length > 0) {
    throw new AddnessCodeApiError(`Unknown parameter(s): ${unknown.join(", ")}`);
  }
  if (definition.requiresForce && input.force !== true) {
    throw new AddnessCodeApiError("This operation requires force: true");
  }

  const args = [...definition.command];
  for (const parameter of definition.parameters) {
    appendParameter(args, parameter, input[parameter.name]);
  }
  args.push("--json");
  return args;
}

function parseStructuredOutput(stdout, operation) {
  const trimmed = stdout.trim();
  if (trimmed === "") return null;
  try {
    return JSON.parse(trimmed);
  } catch {
    const lines = trimmed.split(/\r?\n/).filter(Boolean);
    try {
      const values = lines.map((line) => JSON.parse(line));
      return values.length === 1 ? values[0] : values;
    } catch {
      throw new AddnessCodeApiError(
        `Operation ${operation} returned non-JSON output`,
        { outputBytes: Buffer.byteLength(stdout) },
      );
    }
  }
}

export async function callAddness(definition, input = {}, options = {}) {
  if (options === null || Array.isArray(options) || typeof options !== "object") {
    throw new AddnessCodeApiError("options must be an object");
  }
  const args = buildArguments(definition, input);
  const binary = process.env.ADDNESS_BIN || "addness";
  const timeoutMs = options.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const maxOutputBytes = options.maxOutputBytes ?? DEFAULT_MAX_OUTPUT_BYTES;
  const operation = definition.command.join("/");
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0 || timeoutMs > 2_147_483_647) {
    throw new AddnessCodeApiError("timeoutMs must be a positive 32-bit integer");
  }
  if (!Number.isSafeInteger(maxOutputBytes) || maxOutputBytes <= 0) {
    throw new AddnessCodeApiError("maxOutputBytes must be a positive integer");
  }

  return new Promise((resolve, reject) => {
    const child = spawn(binary, args, {
      cwd: options.cwd ?? process.cwd(),
      env: process.env,
      shell: false,
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
    });
    const stdout = [];
    const stderr = [];
    let outputBytes = 0;
    let terminalError;
    let settled = false;
    let forceKillTimer;

    const terminate = () => {
      child.kill();
      forceKillTimer ??= setTimeout(() => child.kill("SIGKILL"), 1_000);
    };

    const fail = (error) => {
      if (settled) return;
      settled = true;
      reject(error);
    };
    const capture = (target) => (chunk) => {
      outputBytes += chunk.length;
      if (outputBytes > maxOutputBytes) {
        terminalError ??= new AddnessCodeApiError(
          `Operation ${operation} exceeded ${maxOutputBytes} output bytes`,
        );
        terminate();
        return;
      }
      target.push(chunk);
    };

    child.stdout.on("data", capture(stdout));
    child.stderr.on("data", capture(stderr));
    child.on("error", (error) => {
      terminalError = new AddnessCodeApiError(
        `Failed to start ${operation}: ${error.message}`,
      );
    });

    const timer = setTimeout(() => {
      terminalError = new AddnessCodeApiError(
        `Operation ${operation} timed out after ${timeoutMs}ms`,
      );
      terminate();
    }, timeoutMs);

    const abort = () => {
      terminalError = new AddnessCodeApiError(`Operation ${operation} was aborted`);
      terminate();
    };
    if (options.signal) {
      if (options.signal.aborted) abort();
      else options.signal.addEventListener("abort", abort, { once: true });
    }

    child.on("close", (code, signal) => {
      clearTimeout(timer);
      clearTimeout(forceKillTimer);
      options.signal?.removeEventListener("abort", abort);
      if (terminalError) {
        fail(terminalError);
        return;
      }
      const stdoutText = Buffer.concat(stdout).toString("utf8");
      const stderrText = Buffer.concat(stderr).toString("utf8");
      if (code !== 0) {
        fail(new AddnessCodeApiError(`Operation ${operation} failed`, {
          exitCode: code,
          signal,
          stderr: stderrText.slice(-4_000),
        }));
        return;
      }
      try {
        const value = parseStructuredOutput(stdoutText, operation);
        if (!settled) {
          settled = true;
          resolve(value);
        }
      } catch (error) {
        fail(error);
      }
    });
  });
}
