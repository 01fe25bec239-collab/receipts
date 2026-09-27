use super::Migration;

pub(crate) const MIGRATION: Migration = Migration {
    version: 13,
    name: "graph_repository",
    sql: SQL,
};

pub(crate) const SQL: &str = r#"
CREATE TABLE graphs (
    project_id ANY NOT NULL COLLATE BINARY CHECK (typeof(project_id)='text'),
    graph_id ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_id)='text'),
    current_version ANY NOT NULL COLLATE BINARY CHECK (typeof(current_version)='text' AND instr(current_version,char(0))=0 AND length(current_version)>=1 AND current_version NOT GLOB '*[^0-9]*' AND substr(current_version,1,1) BETWEEN '1' AND '9'),
    PRIMARY KEY (project_id, graph_id),
    FOREIGN KEY (project_id, graph_id, current_version)
      REFERENCES graph_versions(project_id, graph_id, graph_version) DEFERRABLE INITIALLY DEFERRED
) STRICT;
CREATE TABLE graph_versions (
    project_id ANY NOT NULL COLLATE BINARY CHECK (typeof(project_id)='text'),
    graph_id ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_id)='text'),
    graph_version ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_version)='text' AND instr(graph_version,char(0))=0 AND length(graph_version)>=1 AND graph_version NOT GLOB '*[^0-9]*' AND substr(graph_version,1,1) BETWEEN '1' AND '9'),
    parent_version_present INTEGER NOT NULL CHECK (parent_version_present IN (0,1)),
    parent_version ANY CHECK (parent_version IS NULL OR (typeof(parent_version)='text' AND instr(parent_version,char(0))=0 AND length(parent_version)>=1 AND parent_version NOT GLOB '*[^0-9]*' AND substr(parent_version,1,1) BETWEEN '1' AND '9')),
    goal_id ANY NOT NULL CHECK (typeof(goal_id)='text'),
    policy_id ANY NOT NULL CHECK (typeof(policy_id)='text'),
    created_at ANY NOT NULL CHECK (typeof(created_at)='text'),
    compiled_from_present INTEGER NOT NULL CHECK (compiled_from_present IN (0,1)),
    context_epoch ANY CHECK (context_epoch IS NULL OR (typeof(context_epoch)='text' AND instr(context_epoch,char(0))=0 AND length(context_epoch)>=1 AND context_epoch NOT GLOB '*[^0-9]*' AND (context_epoch='0' OR substr(context_epoch,1,1) BETWEEN '1' AND '9'))),
    resulting_digest ANY NOT NULL CHECK (typeof(resulting_digest)='text'),
    clock_source_id ANY NOT NULL CHECK (typeof(clock_source_id)='text'),
    clock_contract_version ANY NOT NULL CHECK (typeof(clock_contract_version)='text'),
    compiler_id ANY NOT NULL CHECK (typeof(compiler_id)='text'),
    source_ref ANY NOT NULL CHECK (typeof(source_ref)='text'),
    creation_reason ANY NOT NULL CHECK (typeof(creation_reason)='text'),
    PRIMARY KEY (project_id, graph_id, graph_version),
    FOREIGN KEY (project_id, graph_id) REFERENCES graphs(project_id, graph_id),
    CHECK (parent_version_present=1 OR parent_version IS NULL)
) STRICT;
CREATE TABLE graph_nodes (
    project_id ANY NOT NULL COLLATE BINARY CHECK (typeof(project_id)='text'),
    graph_id ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_id)='text'),
    graph_version ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_version)='text' AND instr(graph_version,char(0))=0 AND length(graph_version)>=1 AND graph_version NOT GLOB '*[^0-9]*' AND substr(graph_version,1,1) BETWEEN '1' AND '9'),
    node_id ANY NOT NULL COLLATE BINARY CHECK (typeof(node_id)='text'),
    kind ANY NOT NULL CHECK (typeof(kind)='text'),
    state ANY NOT NULL CHECK (typeof(state)='text'),
    title ANY CHECK (title IS NULL OR typeof(title)='text'),
    parent_node_id ANY CHECK (parent_node_id IS NULL OR typeof(parent_node_id)='text'),
    attempt_number ANY CHECK (attempt_number IS NULL OR (typeof(attempt_number)='text' AND instr(attempt_number,char(0))=0 AND length(attempt_number)>=1 AND attempt_number NOT GLOB '*[^0-9]*' AND substr(attempt_number,1,1) BETWEEN '1' AND '9')),
    required_capabilities_present INTEGER NOT NULL CHECK (required_capabilities_present IN (0,1)),
    task_capsule_ref ANY CHECK (task_capsule_ref IS NULL OR typeof(task_capsule_ref)='text'),
    workstream_id ANY CHECK (workstream_id IS NULL OR typeof(workstream_id)='text'),
    code_sha ANY CHECK (code_sha IS NULL OR typeof(code_sha)='text'),
    workspace_id ANY CHECK (workspace_id IS NULL OR typeof(workspace_id)='text'),
    result_ref ANY CHECK (result_ref IS NULL OR typeof(result_ref)='text'),
    locked_reason_present INTEGER NOT NULL CHECK (locked_reason_present IN (0,1)),
    locked_reason ANY CHECK (locked_reason IS NULL OR typeof(locked_reason)='text'),
    created_in_version ANY CHECK (created_in_version IS NULL OR (typeof(created_in_version)='text' AND instr(created_in_version,char(0))=0 AND length(created_in_version)>=1 AND created_in_version NOT GLOB '*[^0-9]*' AND substr(created_in_version,1,1) BETWEEN '1' AND '9')),
    PRIMARY KEY (project_id, graph_id, graph_version, node_id),
    FOREIGN KEY (project_id, graph_id, graph_version)
      REFERENCES graph_versions(project_id, graph_id, graph_version),
    CHECK (locked_reason_present=1 OR locked_reason IS NULL)
) STRICT;
CREATE TABLE graph_edges (
    project_id ANY NOT NULL COLLATE BINARY CHECK (typeof(project_id)='text'),
    graph_id ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_id)='text'),
    graph_version ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_version)='text' AND instr(graph_version,char(0))=0 AND length(graph_version)>=1 AND graph_version NOT GLOB '*[^0-9]*' AND substr(graph_version,1,1) BETWEEN '1' AND '9'),
    edge_id ANY NOT NULL COLLATE BINARY CHECK (typeof(edge_id)='text'),
    from_node ANY NOT NULL COLLATE BINARY CHECK (typeof(from_node)='text'),
    to_node ANY NOT NULL COLLATE BINARY CHECK (typeof(to_node)='text'),
    edge_class ANY NOT NULL CHECK (typeof(edge_class)='text'),
    precedence_kind ANY CHECK (precedence_kind IS NULL OR typeof(precedence_kind)='text'),
    control_kind ANY CHECK (control_kind IS NULL OR typeof(control_kind)='text'),
    note ANY CHECK (note IS NULL OR typeof(note)='text'),
    PRIMARY KEY (project_id, graph_id, graph_version, edge_id),
    FOREIGN KEY (project_id, graph_id, graph_version)
      REFERENCES graph_versions(project_id, graph_id, graph_version),
    FOREIGN KEY (project_id, graph_id, graph_version, from_node)
      REFERENCES graph_nodes(project_id, graph_id, graph_version, node_id),
    FOREIGN KEY (project_id, graph_id, graph_version, to_node)
      REFERENCES graph_nodes(project_id, graph_id, graph_version, node_id),
    CHECK ((edge_class='PRECEDENCE' AND precedence_kind IS NOT NULL AND control_kind IS NULL)
        OR (edge_class='CONTROL' AND control_kind IS NOT NULL AND precedence_kind IS NULL))
) STRICT;
CREATE TABLE graph_node_capabilities (
    project_id ANY NOT NULL COLLATE BINARY CHECK (typeof(project_id)='text'),
    graph_id ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_id)='text'),
    graph_version ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_version)='text' AND instr(graph_version,char(0))=0 AND length(graph_version)>=1 AND graph_version NOT GLOB '*[^0-9]*' AND substr(graph_version,1,1) BETWEEN '1' AND '9'),
    node_id ANY NOT NULL COLLATE BINARY CHECK (typeof(node_id)='text'),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    capability ANY NOT NULL CHECK (typeof(capability)='text'),
    PRIMARY KEY (project_id, graph_id, graph_version, node_id, ordinal),
    FOREIGN KEY (project_id, graph_id, graph_version, node_id)
      REFERENCES graph_nodes(project_id, graph_id, graph_version, node_id)
) STRICT;
CREATE TABLE graph_compiled_sources (
    project_id ANY NOT NULL COLLATE BINARY CHECK (typeof(project_id)='text'),
    graph_id ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_id)='text'),
    graph_version ANY NOT NULL COLLATE BINARY CHECK (typeof(graph_version)='text' AND instr(graph_version,char(0))=0 AND length(graph_version)>=1 AND graph_version NOT GLOB '*[^0-9]*' AND substr(graph_version,1,1) BETWEEN '1' AND '9'),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    source ANY NOT NULL CHECK (typeof(source)='text'),
    PRIMARY KEY (project_id, graph_id, graph_version, ordinal),
    FOREIGN KEY (project_id, graph_id, graph_version)
      REFERENCES graph_versions(project_id, graph_id, graph_version)
) STRICT;
INSERT INTO state_schema_version(version, migration_name) VALUES (13, 'graph_repository');
"#;
