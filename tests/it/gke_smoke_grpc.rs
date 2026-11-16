//! The GKE smoke suite runs `sift acceptance-grpc` as an init container of the
//! probe Job, and the smoke server opens port 4317 to that probe only.
//! These checks read the committed YAML; no test command reaches a cluster.
use serde::Deserialize;
use serde_yaml::Value;

const OVERLAY: &str = include_str!("../../k8s/overlays/gke-test/sift.yaml");
const JOB: &str = include_str!("../../k8s/gke-test-job/smoke/job.yaml");
const ACCEPTANCE_GRPC: &str = include_str!("../../src/bin/sift/acceptance_grpc.rs");

fn docs(text: &str) -> Vec<Value> {
    serde_yaml::Deserializer::from_str(text)
        .map(|d| Value::deserialize(d).unwrap())
        .collect()
}

fn doc<'a>(docs: &'a [Value], kind: &str, name: &str) -> Result<&'a Value, String> {
    docs.iter()
        .find(|d| d["kind"].as_str() == Some(kind) && d["metadata"]["name"].as_str() == Some(name))
        .ok_or_else(|| format!("missing {kind} {name}"))
}

fn doc_mut<'a>(docs: &'a mut [Value], kind: &str, name: &str) -> &'a mut Value {
    docs.iter_mut()
        .find(|d| d["kind"].as_str() == Some(kind) && d["metadata"]["name"].as_str() == Some(name))
        .unwrap_or_else(|| panic!("missing {kind} {name}"))
}

fn seq_mut(value: &mut Value) -> &mut Vec<Value> {
    value.as_sequence_mut().expect("sequence")
}

fn check_overlay(docs: &[Value]) -> Result<(), String> {
    let policy = doc(docs, "NetworkPolicy", "sift-smoke")?;
    let rules = policy["spec"]["ingress"]
        .as_sequence()
        .ok_or("network policy has no ingress list")?;
    if rules.len() != 1 {
        return Err(format!(
            "network policy must have one ingress rule, has {}",
            rules.len()
        ));
    }
    let ports = rules[0]["ports"]
        .as_sequence()
        .ok_or("ingress rule has no ports")?;
    let mut opened = Vec::new();
    for port in ports {
        if port["protocol"].as_str() != Some("TCP") {
            return Err(format!("network policy port is not TCP: {port:?}"));
        }
        opened.push(
            port["port"]
                .as_u64()
                .ok_or("network policy port has no number")?,
        );
    }
    opened.sort_unstable();
    if opened != [4317, 7380] {
        return Err(format!(
            "network policy must open only 4317 and 7380, opens {opened:?}"
        ));
    }

    let service = doc(docs, "Service", "sift-smoke")?;
    let service_ports = service["spec"]["ports"]
        .as_sequence()
        .ok_or("service has no ports")?;
    let expected_service = [(7380_u64, "http"), (4317_u64, "grpc")];
    if service_ports.len() != expected_service.len()
        || !service_ports
            .iter()
            .zip(expected_service)
            .all(|(port, (number, target))| {
                port["port"].as_u64() == Some(number) && port["targetPort"].as_str() == Some(target)
            })
    {
        return Err(format!(
            "service must expose 7380 (http) and 4317 (grpc), has {service_ports:?}"
        ));
    }

    let statefulset = doc(docs, "StatefulSet", "sift-smoke")?;
    let containers = statefulset["spec"]["template"]["spec"]["containers"]
        .as_sequence()
        .ok_or("statefulset has no containers")?;
    let sift = containers
        .iter()
        .find(|c| c["name"].as_str() == Some("sift"))
        .ok_or("statefulset has no container sift")?;
    let container_ports = sift["ports"]
        .as_sequence()
        .ok_or("container sift has no ports")?;
    let expected_container = [("http", 7380_u64), ("grpc", 4317_u64)];
    if container_ports.len() != expected_container.len()
        || !container_ports
            .iter()
            .zip(expected_container)
            .all(|(port, (name, number))| {
                port["name"].as_str() == Some(name)
                    && port["containerPort"].as_u64() == Some(number)
            })
    {
        return Err(format!(
            "container sift must expose http 7380 and grpc 4317, has {container_ports:?}"
        ));
    }
    Ok(())
}

fn check_job(docs: &[Value]) -> Result<(), String> {
    let job = doc(docs, "Job", "sift-smoke-probe")?;
    if job["spec"]["backoffLimit"].as_u64() != Some(0) {
        return Err("job must not retry: spec.backoffLimit must be 0".into());
    }
    let pod = &job["spec"]["template"]["spec"];
    if pod["restartPolicy"].as_str() != Some("Never") {
        return Err("job pod restartPolicy must be Never".into());
    }
    if pod["nodeSelector"]["faberline.dev/pool"].as_str() != Some("sift-client") {
        return Err("job pod must run on pool sift-client".into());
    }
    let init = pod["initContainers"]
        .as_sequence()
        .ok_or("job has no init containers")?;
    let names: Vec<&str> = init.iter().filter_map(|c| c["name"].as_str()).collect();
    if names != ["fixture", "collector", "grpc"] {
        return Err(format!(
            "init containers must be fixture, collector, grpc; are {names:?}"
        ));
    }
    let grpc = &init[2];
    if grpc["image"].as_str() != Some("sift") {
        return Err("init container grpc must use image sift".into());
    }
    let args: Vec<&str> = grpc["args"]
        .as_sequence()
        .ok_or("init container grpc has no args")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let expected = [
        "acceptance-grpc",
        "--endpoint",
        "http://sift-smoke:4317",
        "--project",
        "local",
    ];
    if args != expected {
        return Err(format!(
            "init container grpc args must be {expected:?}; are {args:?}"
        ));
    }
    let security = &grpc["securityContext"];
    if security["readOnlyRootFilesystem"].as_bool() != Some(true)
        || security["allowPrivilegeEscalation"].as_bool() != Some(false)
    {
        return Err(
            "init container grpc must be read-only and must not escalate privileges".into(),
        );
    }
    let containers = pod["containers"]
        .as_sequence()
        .ok_or("job has no containers")?;
    if containers.len() != 1 || containers[0]["name"].as_str() != Some("verify") {
        return Err("job must have exactly one container, verify".into());
    }
    Ok(())
}

#[test]
fn overlay_opens_only_http_and_grpc() {
    check_overlay(&docs(OVERLAY)).unwrap();
}

#[test]
fn network_policy_without_4317_fails() {
    let mut d = docs(OVERLAY);
    let ports =
        seq_mut(&mut doc_mut(&mut d, "NetworkPolicy", "sift-smoke")["spec"]["ingress"][0]["ports"]);
    ports.retain(|p| p["port"].as_u64() != Some(4317));
    let err = check_overlay(&d).unwrap_err();
    assert!(err.contains("4317"), "{err}");
}

#[test]
fn network_policy_with_extra_port_fails() {
    let mut d = docs(OVERLAY);
    let ports =
        seq_mut(&mut doc_mut(&mut d, "NetworkPolicy", "sift-smoke")["spec"]["ingress"][0]["ports"]);
    ports.push(serde_yaml::from_str("{protocol: TCP, port: 9999}").unwrap());
    assert!(check_overlay(&d).is_err());
}

#[test]
fn service_without_grpc_port_fails() {
    let mut d = docs(OVERLAY);
    let ports = seq_mut(&mut doc_mut(&mut d, "Service", "sift-smoke")["spec"]["ports"]);
    ports.retain(|p| p["port"].as_u64() != Some(4317));
    assert!(check_overlay(&d).is_err());
}

#[test]
fn container_without_grpc_port_fails() {
    let mut d = docs(OVERLAY);
    let ports = seq_mut(
        &mut doc_mut(&mut d, "StatefulSet", "sift-smoke")["spec"]["template"]["spec"]["containers"]
            [0]["ports"],
    );
    ports.retain(|p| p["name"].as_str() != Some("grpc"));
    assert!(check_overlay(&d).is_err());
}

#[test]
fn job_runs_grpc_acceptance() {
    check_job(&docs(JOB)).unwrap();
}

#[test]
fn job_without_grpc_container_fails() {
    let mut d = docs(JOB);
    let init = seq_mut(
        &mut doc_mut(&mut d, "Job", "sift-smoke-probe")["spec"]["template"]["spec"]
            ["initContainers"],
    );
    init.retain(|c| c["name"].as_str() != Some("grpc"));
    let err = check_job(&d).unwrap_err();
    assert!(err.contains("grpc"), "{err}");
}

#[test]
fn job_grpc_with_wrong_project_fails() {
    let mut d = docs(JOB);
    doc_mut(&mut d, "Job", "sift-smoke-probe")["spec"]["template"]["spec"]["initContainers"][2]
        ["args"][4] = Value::String("other".into());
    assert!(check_job(&d).is_err());
}

#[test]
fn job_grpc_with_token_file_fails() {
    let mut d = docs(JOB);
    let args = seq_mut(
        &mut doc_mut(&mut d, "Job", "sift-smoke-probe")["spec"]["template"]["spec"]
            ["initContainers"][2]["args"],
    );
    args.push(Value::String("--token-file".into()));
    assert!(check_job(&d).is_err());
}

#[test]
fn job_may_not_retry_fails() {
    let mut d = docs(JOB);
    doc_mut(&mut d, "Job", "sift-smoke-probe")["spec"]["backoffLimit"] =
        serde_yaml::from_str("2").unwrap();
    assert!(check_job(&d).is_err());
}

#[test]
fn grpc_acceptance_output_is_pinned() {
    for pinned in [
        r#""signal":"logs""#,
        r#""accepted":1"#,
        r#""rejected":rejected"#,
        r#""compression":"gzip""#,
        "if rejected != 1",
    ] {
        assert!(
            ACCEPTANCE_GRPC.contains(pinned),
            "acceptance_grpc.rs lost {pinned}"
        );
    }
}
