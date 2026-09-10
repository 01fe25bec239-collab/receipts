pub(super) const SQL: &str = "CREATE TABLE install_entitlement_schema_version (
    version INTEGER PRIMARY KEY CHECK (version > 0),
    migration_name TEXT NOT NULL
) STRICT";
