use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("addness-cooldown-{}", uuid::Uuid::new_v4())))
    }

    fn guard(&self, token: &str, url: &str) -> RateLimitGuard {
        RateLimitGuard::in_directory(self.0.clone(), token, url)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cooldown_survives_new_clients_and_separates_credentials_and_servers() {
    let fixture = Fixture::new();
    let first = fixture.guard("secret-token", "https://api.test");
    first.check_at(10_000).unwrap();
    assert_eq!(first.record_at(Some("120"), 10_000).unwrap(), 120);
    let next = fixture.guard("secret-token", "https://api.test/");
    assert!(next.check_at(10_119).is_err());
    next.check_at(10_120).unwrap();
    fixture
        .guard("other-token", "https://api.test")
        .check_at(10_001)
        .unwrap();
    fixture
        .guard("secret-token", "https://other.test")
        .check_at(10_001)
        .unwrap();
    assert!(!first.path.to_string_lossy().contains("secret-token"));
    assert!(
        !fs::read_to_string(first.path)
            .unwrap()
            .contains("secret-token")
    );
}

#[test]
fn retry_after_date_and_backoff_are_respected_without_shortening_existing_cooldowns() {
    let fixture = Fixture::new();
    let guard = fixture.guard("key", "https://api.test");
    let date = httpdate::fmt_http_date(UNIX_EPOCH + std::time::Duration::from_secs(10_900));
    assert_eq!(guard.record_at(Some(&date), 10_000).unwrap(), 900);
    assert_eq!(guard.record_at(Some("2"), 10_001).unwrap(), 899);
    assert_eq!(guard.record_at(None, 10_900).unwrap(), 120);
    assert_eq!(guard.record_at(Some("bad"), 11_020).unwrap(), 240);
    assert_eq!(guard.record_at(None, 20_000).unwrap(), 60);
}

#[test]
fn concurrent_writers_preserve_the_longest_wait() {
    let fixture = Fixture::new();
    let guard = fixture.guard("key", "https://api.test");
    let workers: Vec<_> = [60, 900, 120, 300]
        .into_iter()
        .map(|seconds| {
            let guard = guard.clone();
            std::thread::spawn(move || guard.record_at(Some(&seconds.to_string()), 10_000).unwrap())
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert!(guard.check_at(10_899).is_err());
    guard.check_at(10_900).unwrap();
}

#[test]
fn repeated_limits_back_off_to_one_hour_and_invalid_state_does_not_allow_requests() {
    let fixture = Fixture::new();
    let guard = fixture.guard("key", "https://api.test");
    let mut now = 10_000;
    for expected in [60, 120, 240, 480, 960, 1_920, 3_600, 3_600] {
        assert_eq!(guard.record_at(None, now).unwrap(), expected);
        now += expected;
    }
    fs::write(&guard.path, "{broken").unwrap();
    assert!(
        guard
            .check_at(now)
            .unwrap_err()
            .to_string()
            .contains("Invalid local")
    );
}
