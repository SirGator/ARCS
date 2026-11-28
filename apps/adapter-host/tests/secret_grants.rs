//! Nachweis des Geheimnistresors an der Bedienoberfläche.
//!
//! Die Tests prüfen, dass ein Geheimnis den Tresor nicht verlässt: Weder der
//! Bericht noch das Artefaktnetz noch der Journal dürfen den Wert tragen.

#[path = "support/goals.rs"]
mod goals;
mod support;

use arcs_host::operator_command::{CommandReceipt, OperatorCommand, SecretCommand};
use serde_json::json;

/// Stellt den Tresor im Host ein.
fn with_vault(fixture: &goals::Fixture) {
    let mut host = fixture.read("host.json");
    host["secret_vault"] = json!(fixture.directory.join("secrets.sqlite"));
    fixture.write("host.json", host);
}

#[test]
fn an_operator_registers_a_secret_and_issues_a_scoped_grant() {
    let f = goals::Fixture::new();
    with_vault(&f);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);

    let receipt = OperatorCommand::Secret(SecretCommand::Register {
        value: "device-token-value".into(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the registration returns a receipt");
    let CommandReceipt::Secret(registered) = receipt else {
        panic!("expected a secret receipt");
    };
    let secret_id = registered.secret_id.clone().expect("a secret id");
    assert!(!secret_id.contains("device-token-value"));

    let receipt = OperatorCommand::Secret(SecretCommand::Issue {
        secret_id: secret_id.clone(),
        parameter: "token".into(),
        purpose: "act".into(),
        resource: "simulated.fan:fan.write".into(),
        ttl_seconds: 600,
        max_uses: 1,
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the issue returns a receipt");
    let CommandReceipt::Secret(issued) = receipt else {
        panic!("expected a secret receipt");
    };
    let grant_id = issued.grant_id.clone().expect("a grant id");
    assert_eq!(issued.state.as_deref(), Some("active"));
    assert_eq!(issued.uses, Some(0));
    assert_eq!(issued.max_uses, Some(1));
    assert_eq!(issued.resource.as_deref(), Some("simulated.fan:fan.write"));

    // Neither the receipt nor the artifact network carries the value.
    let report = runtime.report(0).unwrap();
    let rendered = serde_json::to_string(&report).unwrap();
    assert!(!rendered.contains("device-token-value"));
    let mut sequence = 0;
    while let Some(committed) = host.store().committed_after(sequence).unwrap().first() {
        assert!(
            !committed
                .artifact
                .payload
                .to_string()
                .contains("device-token-value"),
            "the secret entered the artifact network"
        );
        sequence = committed.sequence;
    }

    // The grant is visible and can be revoked.
    let receipt = OperatorCommand::Secret(SecretCommand::Inspect {
        grant_id: grant_id.clone(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the inspection returns a receipt");
    let CommandReceipt::Secret(inspected) = receipt else {
        panic!("expected a secret receipt");
    };
    assert_eq!(inspected.state.as_deref(), Some("active"));

    let receipt = OperatorCommand::Secret(SecretCommand::Revoke {
        grant_id: grant_id.clone(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the revocation returns a receipt");
    let CommandReceipt::Secret(revoked) = receipt else {
        panic!("expected a secret receipt");
    };
    assert_eq!(revoked.state.as_deref(), Some("revoked"));
}

#[test]
fn a_host_without_a_vault_knows_no_secret() {
    let f = goals::Fixture::new();
    let mut host = f.host();
    let mut runtime = f.runtime(&host);

    let refused = OperatorCommand::Secret(SecretCommand::Register {
        value: "device-token-value".into(),
    })
    .apply(&mut runtime, &mut host);
    assert!(refused.is_err());
}

#[test]
fn a_grant_is_refused_for_an_unknown_or_unusable_secret() {
    let f = goals::Fixture::new();
    with_vault(&f);
    let mut host = f.host();
    let mut runtime = f.runtime(&host);

    // A grant for a secret that the vault does not know is refused.
    let refused = OperatorCommand::Secret(SecretCommand::Issue {
        secret_id: "secret:does-not-exist".into(),
        parameter: "token".into(),
        purpose: "act".into(),
        resource: "simulated.fan:fan.write".into(),
        ttl_seconds: 600,
        max_uses: 1,
    })
    .apply(&mut runtime, &mut host);
    assert!(refused.is_err());

    let receipt = OperatorCommand::Secret(SecretCommand::Register {
        value: "device-token-value".into(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the registration returns a receipt");
    let CommandReceipt::Secret(registered) = receipt else {
        panic!("expected a secret receipt");
    };
    let secret_id = registered.secret_id.clone().expect("a secret id");

    // A grant without a use or without a lifetime would be unlimited.
    for (ttl_seconds, max_uses) in [(0, 1), (600, 0)] {
        let refused = OperatorCommand::Secret(SecretCommand::Issue {
            secret_id: secret_id.clone(),
            parameter: "token".into(),
            purpose: "act".into(),
            resource: "simulated.fan:fan.write".into(),
            ttl_seconds,
            max_uses,
        })
        .apply(&mut runtime, &mut host);
        assert!(refused.is_err(), "an unlimited grant must be refused");
    }

    // An unknown grant is reported as such and does not fail the inspection.
    let receipt = OperatorCommand::Secret(SecretCommand::Inspect {
        grant_id: "grant:unknown".into(),
    })
    .apply(&mut runtime, &mut host)
    .unwrap()
    .expect("the inspection returns a receipt");
    let CommandReceipt::Secret(inspected) = receipt else {
        panic!("expected a secret receipt");
    };
    assert!(inspected.grant_id.is_none());

    // The vault stays the only place that knows the value. The host offers no
    // way to read it back.
    let rendered = serde_json::to_string(
        &host
            .secret_grant("grant:secret-1-act-simulated.fan-fan.write")
            .unwrap(),
    )
    .unwrap();
    assert!(!rendered.contains("device-token-value"));
}
