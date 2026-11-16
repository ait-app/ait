use super::*;

#[test]
fn native_wallets_keep_currency_zero_debt_and_unknown_fields_private() {
    let response = json!({"status":"ready","value":[{"currency":"CNY","balance":"0"},{"currency":"USD","balance":"-0.01"}],"bonusWallets":[],"token":"fixture-secret"});
    let report = project(&response).unwrap();
    assert_eq!(report["balances"][0]["remaining"], 0.0);
    assert_eq!(report["balances"][1]["remaining"], -0.01);
    assert!(!report.to_string().contains("fixture-secret"));
    assert_eq!(report["status"], "available");
}

#[test]
fn unavailable_native_accounts_do_not_invent_balances_or_plan_limits() {
    for response in [
        json!(null),
        json!({"status":"ready","value":[],"bonusWallets":[]}),
    ] {
        let report = project(&response).unwrap();
        assert_eq!(report["status"], "unavailable");
        assert_eq!(report["balances"], json!([]));
        assert_eq!(report["windows"], json!([]));
    }
    assert!(project(&json!({"status":"failed","error":"fixture-secret"})).is_err());
}

#[test]
fn malformed_native_wallets_are_rejected_instead_of_silently_omitted() {
    for response in [
        json!({"status":"ready","value":[]}),
        json!({"status":"ready","value":[{"currency":"EUR","balance":"1"}],"bonusWallets":[]}),
        json!({"status":"ready","value":[{"currency":"CNY","balance":1}],"bonusWallets":[]}),
        json!({"status":"ready","value":[{"currency":"CNY","balance":"1"},{"currency":"CNY","balance":"2"}],"bonusWallets":[]}),
        json!({"status":"ready","value":[{}, {}, {}],"bonusWallets":[]}),
    ] {
        assert!(project(&response).is_err());
    }
    for value in ["", "NaN", "inf", "1e999", "9007199254740992", " 1", "1\n"] {
        assert!(amount(&json!(value)).is_err(), "{value:?}");
    }
    assert!((amount(&json!("1.25e1")).unwrap() - 12.5).abs() < f64::EPSILON);
}
