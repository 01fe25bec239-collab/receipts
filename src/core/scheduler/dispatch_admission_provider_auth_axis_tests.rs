use super::{DispatchAdmissionAxisResult, DispatchAdmissionProviderAuthAxisResult};

#[test]
fn every_result_preserves_independent_open_string_contents() {
    let long_text = " Arbitrary Provider/Status: 未知 🦀 e\u{301}\t\n".repeat(201);
    let contents = [
        None,
        Some(""),
        Some(" "),
        Some(" \t\r\n\u{2003} "),
        Some("  Unrecognized Provider/Status: MiXeD!?\0  "),
        Some("  e\u{301} É 未知 🦀  "),
        Some(long_text.as_str()),
    ];

    for (result, canonical) in [
        (DispatchAdmissionAxisResult::Pass, "PASS"),
        (DispatchAdmissionAxisResult::Fail, "FAIL"),
        (DispatchAdmissionAxisResult::NotApplicable, "NOT_APPLICABLE"),
    ] {
        for provider_id in contents {
            for technical_status in contents {
                let record = DispatchAdmissionProviderAuthAxisResult::new(
                    result,
                    provider_id.map(str::to_owned),
                    technical_status.map(str::to_owned),
                );

                assert_eq!(record.result(), result);
                assert_eq!(record.result().as_str(), canonical);
                assert_eq!(record.provider_id(), provider_id);
                assert_eq!(record.technical_status(), technical_status);
            }
        }
    }
}
