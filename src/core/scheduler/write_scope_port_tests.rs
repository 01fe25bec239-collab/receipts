use super::write_scope_port::*;

struct Fixed(WriteScopeOverlapV1);
impl WriteScopePort for Fixed {
    fn overlap(&self, _: &[String], _: &[String]) -> WriteScopeOverlapV1 {
        self.0.clone()
    }
}

#[test]
fn only_proven_disjointness_admits_concurrent_writers() {
    let scope = vec!["src/**".to_string()];
    assert_eq!(
        classify_write_collision(&Fixed(WriteScopeOverlapV1::Disjoint), &scope, &scope),
        WriteScopeCollisionV1::Concurrent
    );
    assert_eq!(
        classify_write_collision(
            &Fixed(WriteScopeOverlapV1::Overlaps {
                witness: "src/a".into()
            }),
            &scope,
            &scope
        ),
        WriteScopeCollisionV1::Conflict {
            witness: "src/a".into()
        }
    );
    assert_eq!(
        classify_write_collision(&Fixed(WriteScopeOverlapV1::Undetermined), &scope, &scope),
        WriteScopeCollisionV1::Defer
    );
}
