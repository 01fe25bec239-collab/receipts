use super::Migration;
pub(crate) const MIGRATION: Migration = Migration {
    version: 15,
    name: "graph_history",
    sql: SQL,
};
pub(crate) const SQL: &str = r#"
CREATE TABLE graph_node_results (
    project_id TEXT NOT NULL COLLATE BINARY,
    graph_id TEXT NOT NULL COLLATE BINARY,
    result_ref TEXT NOT NULL COLLATE BINARY CHECK (length(CAST(result_ref AS BLOB))>0),
    graph_version TEXT NOT NULL COLLATE BINARY,
    node_id TEXT NOT NULL COLLATE BINARY,
    result_json TEXT NOT NULL,
    PRIMARY KEY (project_id,graph_id,result_ref),
    FOREIGN KEY (project_id,graph_id,graph_version,node_id) REFERENCES graph_nodes(project_id,graph_id,graph_version,node_id)
) STRICT;
CREATE TABLE graph_mutations (
    project_id TEXT NOT NULL COLLATE BINARY,
    graph_id TEXT NOT NULL COLLATE BINARY,
    mutation_id TEXT NOT NULL COLLATE BINARY,
    parent_version TEXT NOT NULL COLLATE BINARY,
    resulting_version TEXT NOT NULL COLLATE BINARY,
    mutation_json TEXT NOT NULL,
    PRIMARY KEY (project_id,graph_id,mutation_id),
    UNIQUE (project_id,graph_id,resulting_version),
    FOREIGN KEY (project_id,graph_id,parent_version) REFERENCES graph_versions(project_id,graph_id,graph_version),
    FOREIGN KEY (project_id,graph_id,resulting_version) REFERENCES graph_versions(project_id,graph_id,graph_version)
) STRICT;
CREATE TABLE graph_snapshot_seals (
    legacy INTEGER NOT NULL DEFAULT 0 CHECK(legacy IN (0,1)),
    project_id TEXT NOT NULL COLLATE BINARY,
    graph_id TEXT NOT NULL COLLATE BINARY,
    graph_version TEXT NOT NULL COLLATE BINARY,
    PRIMARY KEY(project_id,graph_id,graph_version),
    FOREIGN KEY(project_id,graph_id,graph_version) REFERENCES graph_versions(project_id,graph_id,graph_version)
) STRICT;
INSERT INTO graph_snapshot_seals(legacy,project_id,graph_id,graph_version) SELECT 1,project_id,graph_id,graph_version FROM graph_versions;
CREATE TRIGGER graph_snapshot_seals_immutable_update BEFORE UPDATE ON graph_snapshot_seals BEGIN SELECT RAISE(ABORT,'immutable snapshot seal'); END;
CREATE TRIGGER graph_snapshot_seals_immutable_delete BEFORE DELETE ON graph_snapshot_seals BEGIN SELECT RAISE(ABORT,'immutable snapshot seal'); END;
CREATE TRIGGER graph_snapshot_seals_immutable_replace BEFORE INSERT ON graph_snapshot_seals WHEN EXISTS (SELECT 1 FROM graph_snapshot_seals WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version) BEGIN SELECT RAISE(ABORT,'duplicate snapshot seal'); END;
CREATE TRIGGER graph_nodes_immutable_late BEFORE INSERT ON graph_nodes WHEN EXISTS (SELECT 1 FROM graph_snapshot_seals WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version) BEGIN SELECT RAISE(ABORT,'sealed snapshot'); END;
CREATE TRIGGER graph_edges_immutable_late BEFORE INSERT ON graph_edges WHEN EXISTS (SELECT 1 FROM graph_snapshot_seals WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version) BEGIN SELECT RAISE(ABORT,'sealed snapshot'); END;
CREATE TRIGGER graph_node_capabilities_immutable_late BEFORE INSERT ON graph_node_capabilities WHEN EXISTS (SELECT 1 FROM graph_snapshot_seals WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version) BEGIN SELECT RAISE(ABORT,'sealed snapshot'); END;
CREATE TRIGGER graph_compiled_sources_immutable_late BEFORE INSERT ON graph_compiled_sources WHEN EXISTS (SELECT 1 FROM graph_snapshot_seals WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version) BEGIN SELECT RAISE(ABORT,'sealed snapshot'); END;
CREATE TRIGGER graph_versions_immutable_update BEFORE UPDATE ON graph_versions BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_versions_immutable_delete BEFORE DELETE ON graph_versions BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_versions_immutable_replace BEFORE INSERT ON graph_versions WHEN EXISTS (SELECT 1 FROM graph_versions WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version) BEGIN SELECT RAISE(ABORT,'duplicate graph history'); END;
CREATE TRIGGER graph_nodes_immutable_update BEFORE UPDATE ON graph_nodes BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_nodes_immutable_delete BEFORE DELETE ON graph_nodes BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_nodes_immutable_replace BEFORE INSERT ON graph_nodes WHEN EXISTS (SELECT 1 FROM graph_nodes WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version AND node_id=NEW.node_id) BEGIN SELECT RAISE(ABORT,'duplicate graph history'); END;
CREATE TRIGGER graph_edges_immutable_update BEFORE UPDATE ON graph_edges BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_edges_immutable_delete BEFORE DELETE ON graph_edges BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_edges_immutable_replace BEFORE INSERT ON graph_edges WHEN EXISTS (SELECT 1 FROM graph_edges WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version AND edge_id=NEW.edge_id) BEGIN SELECT RAISE(ABORT,'duplicate graph history'); END;
CREATE TRIGGER graph_node_capabilities_immutable_update BEFORE UPDATE ON graph_node_capabilities BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_node_capabilities_immutable_delete BEFORE DELETE ON graph_node_capabilities BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_node_capabilities_immutable_replace BEFORE INSERT ON graph_node_capabilities WHEN EXISTS (SELECT 1 FROM graph_node_capabilities WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version AND node_id=NEW.node_id AND ordinal=NEW.ordinal) BEGIN SELECT RAISE(ABORT,'duplicate graph history'); END;
CREATE TRIGGER graph_compiled_sources_immutable_update BEFORE UPDATE ON graph_compiled_sources BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_compiled_sources_immutable_delete BEFORE DELETE ON graph_compiled_sources BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_compiled_sources_immutable_replace BEFORE INSERT ON graph_compiled_sources WHEN EXISTS (SELECT 1 FROM graph_compiled_sources WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.graph_version AND ordinal=NEW.ordinal) BEGIN SELECT RAISE(ABORT,'duplicate graph history'); END;
CREATE TRIGGER graph_node_results_immutable_update BEFORE UPDATE ON graph_node_results BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_node_results_immutable_delete BEFORE DELETE ON graph_node_results BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_node_results_immutable_replace BEFORE INSERT ON graph_node_results WHEN EXISTS (SELECT 1 FROM graph_node_results WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND result_ref=NEW.result_ref) BEGIN SELECT RAISE(ABORT,'duplicate graph history'); END;
CREATE TRIGGER graph_mutations_immutable_late BEFORE INSERT ON graph_mutations WHEN EXISTS (SELECT 1 FROM graph_snapshot_seals WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id AND graph_version=NEW.resulting_version) BEGIN SELECT RAISE(ABORT,'sealed mutation'); END;
CREATE TRIGGER graph_mutations_immutable_update BEFORE UPDATE ON graph_mutations BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_mutations_immutable_delete BEFORE DELETE ON graph_mutations BEGIN SELECT RAISE(ABORT,'immutable graph history'); END;
CREATE TRIGGER graph_mutations_immutable_replace BEFORE INSERT ON graph_mutations WHEN EXISTS (SELECT 1 FROM graph_mutations WHERE (project_id=NEW.project_id AND graph_id=NEW.graph_id AND mutation_id=NEW.mutation_id) OR (project_id=NEW.project_id AND graph_id=NEW.graph_id AND resulting_version=NEW.resulting_version)) BEGIN SELECT RAISE(ABORT,'duplicate graph history'); END;
CREATE TRIGGER graphs_immutable_delete BEFORE DELETE ON graphs BEGIN SELECT RAISE(ABORT,'immutable graph identity'); END;
CREATE TRIGGER graphs_immutable_replace BEFORE INSERT ON graphs WHEN EXISTS (SELECT 1 FROM graphs WHERE project_id=NEW.project_id AND graph_id=NEW.graph_id) BEGIN SELECT RAISE(ABORT,'duplicate graph identity'); END;
CREATE TRIGGER graphs_immutable_identity BEFORE UPDATE OF project_id,graph_id ON graphs BEGIN SELECT RAISE(ABORT,'immutable graph identity'); END;
INSERT INTO state_schema_version(version,migration_name) VALUES (15,'graph_history');
"#;
