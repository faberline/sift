# Sift threat model

This page lists each trust boundary of Sift, the control at each one, and the
test or CI job that proves it. Every row names a proof. The check is
`scripts/security/threat_model.py`.

## Trust boundaries

| Boundary | Control | Proof |
|---|---|---|
| Client to API | A data-plane request needs a bearer token. A token is bound to one project, and a read-only token cannot write. A reader gets 403 on management writes. With Kubernetes auth, a denied SubjectAccessReview returns a forbidden response. | `tests/it/runtime_security_e2e.rs::a_wrong_bearer_token_is_rejected_with_401`<br>`tests/it/runtime_security_e2e.rs::a_read_only_token_cannot_write`<br>`tests/it/runtime_security_e2e.rs::a_token_is_bound_to_its_project`<br>`tests/it/kubernetes_auth.rs::a_subject_access_review_deny_is_a_forbidden_response`<br>`tests/it/control_admin_rights.rs::reader_gets_403_on_every_management_write` |
| TLS ingress | The public API is served over HTTPS only, through the GKE Ingress with a Google-managed certificate. Plain HTTP is refused at the load balancer (see `docs/operations/public-tls.md`). Raft peer traffic uses mutual TLS on port 7381 (see `docs/operations/ha.md`). | `scripts/verify-deployment-artifacts.py::check_public_tls_rejects_plain_http`<br>`src/app/peer_tls_reload/tests.rs::peer_tls_rotation_keeps_requests_ok_and_uses_the_new_certificate` |
| Webhook receiver | An alert webhook destination must be an allowed HTTPS host. The secret reference is checked before any network use. | `src/alerting/infrastructure/webhook/tests.rs::a_destination_outside_the_https_allowlist_is_rejected`<br>`src/alerting/infrastructure/webhook/tests.rs::a_bad_reference_or_file_is_rejected_before_any_network_use` |
| Operator to Kubernetes API | The operator runs under its own ServiceAccount, bound to one ClusterRole. The deployment check reads the operator manifests, checks that the operator, the binding and the role agree, and checks that the operator image is pinned by digest. | `scripts/verify-deployment-artifacts.py::validate_operator` |
| Storage and object store | The archive manifest is written last, and a cold restore equals the source. A failed archive write never compacts the WAL. | `tests/gcs_archive.rs::vat_gcs_archive_manifest_is_written_last_and_cold_restore_is_equal`<br>`tests/gcs_archive.rs::archive_failure_never_compacts_the_wal` |
| Image supply chain | Dependencies come only from crates.io and the Faberline git sources listed in `deny.toml`, checked by `cargo deny check bans sources`. The release image built from the same binary has the same digest on two builds. | `.github/workflows/ci.yml::test-security`<br>`.github/workflows/ci.yml::test-reproducible-image`<br>`tests/it/reproducible_image_script.rs::different_digests_fail` |
| Secrets | The webhook Secret and the peer TLS Secret are read again when their files change. Rotating them causes no failed request. | `src/alerting/infrastructure/webhook/tests.rs::webhook_secret_rotation_is_used_by_the_next_read_and_never_fails_a_read`<br>`src/app/peer_tls_reload/tests.rs::peer_tls_rotation_keeps_requests_ok_and_uses_the_new_certificate` |

## Known gaps

- The CI job `test-security` runs `cargo deny check bans sources` and the
  architecture check. `cargo deny check advisories`, `cargo deny check licenses`
  and `cargo audit` are not in CI yet (issue #139).

## Independent security review

An independent security review is a person's step. It is not claimed: no
review has been done. The table above is evidence from tests and CI, not a
review.
