use super::*;

fn webhook(dir: &std::path::Path) -> ConfiguredWebhook {
    ConfiguredWebhook::new(
        dir.to_path_buf(),
        BTreeSet::from(["hooks.example.com".to_string()]),
    )
    .unwrap()
}

fn secret(dir: &std::path::Path, reference: &str, json: &str) {
    std::fs::write(dir.join(format!("{reference}.json")), json).unwrap();
}

fn error(webhook: &ConfiguredWebhook, reference: &str) -> String {
    webhook
        .destination(reference)
        .err()
        .expect("must be rejected")
}

#[test]
fn a_valid_secret_with_an_allowed_https_host_is_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let config = webhook(dir.path());
    secret(
        dir.path(),
        "hook",
        r#"{"url":"https://hooks.example.com/notify","bearer_token":"tok"}"#,
    );
    secret(
        dir.path(),
        "plain",
        r#"{"url":"https://hooks.example.com/x"}"#,
    );

    let destination = config.destination("hook").unwrap();
    assert_eq!(destination.url, "https://hooks.example.com/notify");
    assert_eq!(destination.bearer_token, Some("tok".to_string()));

    let plain = config.destination("plain").unwrap();
    assert_eq!(plain.bearer_token, None);
}

#[test]
fn a_bad_reference_or_file_is_rejected_before_any_network_use() {
    let dir = tempfile::tempdir().unwrap();
    let config = webhook(dir.path());
    secret(
        dir.path(),
        "hook",
        r#"{"url":"https://hooks.example.com/x"}"#,
    );

    for reference in ["", "../hook", "a/b", "a.b"] {
        assert_eq!(
            error(&config, reference),
            "invalid Webhook Secret reference"
        );
    }
    assert_eq!(error(&config, "missing"), "Webhook Secret is unavailable");

    std::fs::write(dir.path().join("big.json"), vec![b' '; 16 * 1024 + 1]).unwrap();
    assert_eq!(error(&config, "big"), "Webhook Secret exceeds 16 KiB");

    secret(
        dir.path(),
        "extra",
        r#"{"url":"https://hooks.example.com/x","extra":1}"#,
    );
    assert_eq!(error(&config, "extra"), "Webhook Secret is invalid");

    secret(dir.path(), "junk", "not json");
    assert_eq!(error(&config, "junk"), "Webhook Secret is invalid");
}

#[test]
fn a_destination_outside_the_https_allowlist_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let config = webhook(dir.path());
    let cases = [
        ("unknown_host", "https://other.example.com/x"),
        ("http", "http://hooks.example.com/x"),
        ("port", "https://hooks.example.com:8443/x"),
        ("userinfo", "https://user:pw@hooks.example.com/x"),
        ("fragment", "https://hooks.example.com/x#frag"),
    ];
    for (name, url) in cases {
        secret(dir.path(), name, &format!(r#"{{"url":"{url}"}}"#));
        assert_eq!(
            error(&config, name),
            "Webhook destination is outside the HTTPS host allowlist",
            "{name}"
        );
    }

    secret(dir.path(), "badurl", r#"{"url":"not a url"}"#);
    assert_eq!(
        error(&config, "badurl"),
        "Webhook destination URL is invalid"
    );
}

#[test]
fn webhook_secret_rotation_is_used_by_the_next_read_and_never_fails_a_read() {
    let dir = tempfile::tempdir().unwrap();
    let config = webhook(dir.path());
    let old = r#"{"url":"https://hooks.example.com/old","bearer_token":"old"}"#;
    let new = r#"{"url":"https://hooks.example.com/new","bearer_token":"new"}"#;

    // Negative check: a bad body is rejected, so a failed read is visible.
    secret(dir.path(), "hook", "not json");
    assert_eq!(error(&config, "hook"), "Webhook Secret is invalid");
    secret(dir.path(), "hook", old);

    // Rotate by write-then-rename, as the kubelet swaps a mounted file.
    let writer_dir = dir.path().to_path_buf();
    let writer = std::thread::spawn(move || {
        for i in 0..200 {
            let body = if i % 2 == 0 { old } else { new };
            std::fs::write(writer_dir.join("hook.json.tmp"), body).unwrap();
            std::fs::rename(
                writer_dir.join("hook.json.tmp"),
                writer_dir.join("hook.json"),
            )
            .unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    });

    let mut failed = 0;
    for _ in 0..200 {
        match config.destination("hook") {
            Ok(destination) => assert!(
                destination.url == "https://hooks.example.com/old"
                    || destination.url == "https://hooks.example.com/new",
                "unexpected destination {}",
                destination.url
            ),
            Err(_) => failed += 1,
        }
    }
    writer.join().unwrap();

    assert_eq!(failed, 0, "no read may fail during rotation");
    let last = config.destination("hook").unwrap();
    assert_eq!(last.url, "https://hooks.example.com/new");
    assert_eq!(last.bearer_token, Some("new".to_string()));
}
