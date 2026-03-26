use baton::config::normalise_cron;

#[test]
fn normalise_5_field_cron() {
    let result = normalise_cron("0 2 * * *");
    assert_eq!(result, "0 0 2 * * * *");
}

#[test]
fn normalise_6_field_cron() {
    let result = normalise_cron("0 2 * * * 2026");
    assert_eq!(result, "0 0 2 * * * 2026");
}

#[test]
fn normalise_7_field_passthrough() {
    let result = normalise_cron("0 0 2 * * * *");
    assert_eq!(result, "0 0 2 * * * *");
}

#[test]
fn every_minute() {
    let result = normalise_cron("* * * * *");
    assert_eq!(result, "0 * * * * * *");
}

#[test]
fn complex_expression() {
    let result = normalise_cron("*/15 9-17 * * 1-5");
    assert_eq!(result, "0 */15 9-17 * * 1-5 *");
}
