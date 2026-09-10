pub(super) const SQL: &str = "CREATE TABLE install_activation_state (
    singleton_id INTEGER PRIMARY KEY CHECK (singleton_id = 1),
    activation_state TEXT NOT NULL,
    subject_id TEXT,
    first_activated_at TEXT,
    last_known_tier_id TEXT,
    last_entitlement_seen_at TEXT,
    last_observed_server_time TEXT,
    logged_out_at TEXT,
    recorded_at TEXT NOT NULL
) STRICT";
