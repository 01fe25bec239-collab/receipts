pub(super) const SQL: &str = "CREATE TABLE install_entitlement_cache (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    raw_document BLOB NOT NULL
) STRICT";
