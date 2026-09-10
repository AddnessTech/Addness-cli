//! User questions are separate from tool approvals: no default or timer answers them.
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Questions {
    pub(super) thread_id: String,
    pub(super) turn_id: String,
    pub(super) item_id: String,
    #[serde(default = "blocking_default")]
    pub(super) is_blocking: bool,
    pub(super) questions: Vec<Question>,
    #[serde(skip)]
    pub(super) id: Value,
    #[serde(skip)]
    answers: Map<String, Value>,
}

fn blocking_default() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Question {
    pub(super) id: String,
    pub(super) header: String,
    pub(super) question: String,
    #[serde(default)]
    pub(super) is_other: bool,
    #[serde(default)]
    pub(super) is_secret: bool,
    pub(super) options: Option<Vec<Choice>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub(super) struct Choice {
    pub(super) label: String,
    pub(super) description: String,
}

impl Questions {
    pub(super) fn parse(id: Value, params: &Value) -> Result<Self, String> {
        let mut request: Self =
            serde_json::from_value(params.clone()).map_err(|e| e.to_string())?;
        if request.questions.is_empty()
            || request.questions.iter().enumerate().any(|(i, q)| {
                q.id.is_empty()
                    || request.questions[..i]
                        .iter()
                        .any(|previous| previous.id == q.id)
            })
        {
            return Err("questions must have nonempty, unique ids".to_string());
        }
        request.id = id;
        Ok(request)
    }

    pub(super) fn current(&self) -> Option<&Question> {
        self.questions
            .iter()
            .find(|q| !self.answers.contains_key(&q.id))
    }

    pub(super) fn prompt(&self) -> String {
        let Some(q) = self.current() else {
            return "回答を送信中".to_string();
        };
        let mode = if self.is_blocking {
            "回答待ち"
        } else {
            "任意の確認・作業継続中"
        };
        let mut text = format!(
            "Codex 質問（{mode}）{}/{} [{}]\n{}",
            self.answers.len() + 1,
            self.questions.len(),
            q.header,
            q.question
        );
        for (i, option) in q.options.as_deref().unwrap_or_default().iter().enumerate() {
            text.push_str(&format!(
                "\n{}. {} — {}",
                i + 1,
                option.label,
                option.description
            ));
        }
        text.push_str(
            "\n/answer <番号または回答> で回答、/answer --cancel で取消、/answer で再表示",
        );
        if q.is_secret {
            text.push_str("（回答は画面で伏字・履歴に保存しません）");
        }
        text
    }

    /// Advance only after explicit, valid input. Return true once all answers are ready.
    pub(super) fn answer(&mut self, text: &str) -> Result<bool, &'static str> {
        let Some(q) = self.current() else {
            return Ok(true);
        };
        let choices = q.options.as_deref().unwrap_or_default();
        let selected = text
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|i| choices.get(i))
            .or_else(|| choices.iter().find(|o| o.label == text));
        let answer = if let Some(selected) = selected {
            selected.label.clone()
        } else if !text.trim().is_empty() && (choices.is_empty() || q.is_other) {
            text.to_string()
        } else {
            return Err("表示された選択肢の番号かラベルを指定してください");
        };
        self.answers
            .insert(q.id.clone(), json!({"answers": [answer]}));
        Ok(self.current().is_none())
    }

    pub(super) fn result(&self, cancel: bool) -> Value {
        let answers = if cancel {
            Map::new()
        } else {
            self.answers.clone()
        };
        json!({"jsonrpc":"2.0", "id":self.id, "result":{"answers":answers}})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn choices_free_text_and_cancel_keep_question_ids() {
        let mut request = Questions::parse(json!("request-1"), &json!({
            "threadId":"t", "turnId":"turn", "itemId":"i", "isBlocking":false,
            "questions":[
                {"id":"choice", "header":"方針", "question":"どちら？", "options":[{"label":"A", "description":"説明"}]},
                {"id":"text", "header":"詳細", "question":"補足？", "isSecret":true}
            ]
        })).unwrap();
        assert!(!request.is_blocking);
        assert!(request.answer("invalid").is_err());
        assert!(!request.answer("1").unwrap());
        assert!(request.current().unwrap().is_secret);
        assert!(request.answer("自由な回答").unwrap());
        assert_eq!(
            request.result(false)["result"]["answers"],
            json!({"choice":{"answers":["A"]},"text":{"answers":["自由な回答"]}})
        );
        assert_eq!(request.result(true)["result"], json!({"answers":{}}));
        assert_eq!(request.result(false)["id"], "request-1");
    }
    #[test]
    fn optional_freeform_and_legacy_blocking_do_not_auto_answer() {
        let params = json!({"threadId":"t","turnId":"u","itemId":"i","autoResolutionMs":1,"questions":[{"id":"q","header":"h","question":"?","isOther":true,"options":[{"label":"A","description":"a"}]}]});
        let mut request = Questions::parse(json!(12), &params).unwrap();
        assert!(request.is_blocking);
        assert_eq!(request.result(false)["result"], json!({"answers":{}}));
        assert!(request.answer("別の方法").unwrap());
        let mut duplicate = params;
        let q = duplicate["questions"][0].clone();
        duplicate["questions"].as_array_mut().unwrap().push(q);
        assert!(Questions::parse(json!(1), &duplicate).is_err());
    }
}
