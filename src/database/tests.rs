use super::*;

fn memory(engine: Engine) -> DatabaseSession {
    DatabaseSession::connect(&ConnectionConfig {
        engine,
        read_only: false,
        ..ConnectionConfig::default()
    })
    .unwrap()
}
fn run(session: &DatabaseSession, sql: &str) -> QueryOutput {
    session.execute(sql, 100, &CancelToken::new()).unwrap()
}

#[test]
fn embedded_engines_execute_scripts_preserve_nulls_and_describe_schema() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let session = memory(engine);
        let output = run(
            &session,
            "CREATE TABLE items (id INTEGER PRIMARY KEY, title VARCHAR NOT NULL, optional INTEGER); INSERT INTO items VALUES (1, 'semi;colon', NULL), (2, 'second', 3); SELECT * FROM items ORDER BY id;",
        );
        assert_eq!(output.results.len(), 3);
        assert_eq!(output.results[1].affected_rows, 2);
        assert_eq!(
            output.results[2].rows[0],
            [Some("1".into()), Some("semi;colon".into()), None]
        );
        let objects = session.catalog().unwrap();
        let object = objects
            .iter()
            .find(|object| object.name == "items")
            .unwrap();
        let columns = session.describe(object).unwrap();
        assert_eq!(columns.len(), 3);
        assert!(columns[0].primary_key);
        assert!(!columns[1].nullable);
        assert!(columns[2].nullable);
        let quoted = session.quote_object(object);
        assert_eq!(
            run(&session, &format!("SELECT COUNT(*) FROM {quoted}")).results[0].rows[0][0]
                .as_deref(),
            Some("2")
        );
        run(
            &session,
            "BEGIN; INSERT INTO items VALUES (3, 'rolled back', NULL); ROLLBACK;",
        );
        assert_eq!(
            run(&session, "SELECT COUNT(*) FROM items").results[0].rows[0][0].as_deref(),
            Some("2")
        );
        let limited = session
            .execute("SELECT * FROM items ORDER BY id", 1, &CancelToken::new())
            .unwrap();
        assert_eq!(limited.results[0].rows.len(), 1);
        assert!(limited.results[0].truncated);
        assert!(
            session
                .execute("SELECT * FROM nonexistent", 100, &CancelToken::new())
                .is_err()
        );
    }
}

#[test]
fn sqlite_read_only_is_enforced_by_engine_authorizer() {
    let session = DatabaseSession::connect(&ConnectionConfig {
        read_only: true,
        ..Default::default()
    })
    .unwrap();
    assert!(
        session
            .execute("CREATE TABLE forbidden (id INT)", 10, &CancelToken::new())
            .is_err()
    );
    assert!(
        session
            .execute("PRAGMA query_only = OFF", 10, &CancelToken::new())
            .is_err()
    );
    assert_eq!(
        run(&session, "SELECT 1").results[0].rows[0][0].as_deref(),
        Some("1")
    );
    assert!(session.catalog().unwrap().is_empty());
}

#[test]
fn embedded_queries_can_be_cancelled_and_connection_recovers() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let session = memory(engine);
        let cancel = CancelToken::new();
        let worker_session = session.clone();
        let worker_cancel = cancel.clone();
        let sql = if engine == Engine::SQLite {
            "WITH RECURSIVE counter(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM counter WHERE x<1000000000) SELECT SUM(x) FROM counter"
        } else {
            "SELECT SUM(i*j) FROM range(1000000000) a(i) CROSS JOIN range(1000000000) b(j)"
        };
        let started = Instant::now();
        let worker = std::thread::spawn(move || worker_session.execute(sql, 10, &worker_cancel));
        std::thread::sleep(Duration::from_millis(150));
        cancel.cancel();
        assert!(worker.join().unwrap().is_err());
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            run(&session, "SELECT 42").results[0].rows[0][0].as_deref(),
            Some("42")
        );
    }
}

#[test]
fn sqlite_trigger_body_is_a_single_statement() {
    let session = memory(Engine::SQLite);
    run(
        &session,
        "CREATE TABLE items(id INT, value INT); CREATE TRIGGER increment AFTER INSERT ON items BEGIN UPDATE items SET value = CASE WHEN value = 1 THEN 2 ELSE 3 END; UPDATE items SET value = value + 1; END; INSERT INTO items VALUES(1,1);",
    );
    assert_eq!(
        run(&session, "SELECT value FROM items").results[0].rows[0][0].as_deref(),
        Some("3")
    );
}

#[test]
fn passwords_are_never_serialized_or_debugged_and_invalid_server_settings_fail() {
    let config = ConnectionConfig {
        password: "test-secret-do-not-persist".into(),
        ..Default::default()
    };
    assert!(
        !serde_json::to_string(&config)
            .unwrap()
            .contains(&config.password)
    );
    assert!(!format!("{config:?}").contains(&config.password));
    for engine in [Engine::Postgres, Engine::MySql] {
        let invalid = ConnectionConfig {
            engine,
            read_only: false,
            port: 0,
            ..Default::default()
        };
        assert!(DatabaseSession::connect(&invalid).is_err());
    }
    assert_eq!(Engine::Postgres.quote_identifier("a\"b"), "\"a\"\"b\"");
    assert_eq!(Engine::MySql.quote_identifier("a`b"), "`a``b`");
}

#[test]
fn session_handles_are_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DatabaseSession>();
}

#[test]
fn explain_rejects_scripts_and_does_not_execute_mutations() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let session = memory(engine);
        run(&session, "CREATE TABLE items(id INTEGER)");
        assert!(explain_sql("SELECT 1; INSERT INTO items VALUES(1)", engine).is_err());
        let explain = explain_sql("INSERT INTO items VALUES(1)", engine).unwrap();
        run(&session, &explain);
        assert_eq!(
            run(&session, "SELECT COUNT(*) FROM items").results[0].rows[0][0].as_deref(),
            Some("0")
        );
    }
}

#[test]
fn result_budget_accounts_for_null_and_empty_cell_storage() {
    let mut budget = ResultBudget {
        row_limit: 100,
        remaining_bytes: 128,
    };
    let mut result = QueryResult::default();
    budget.push(&mut result, vec![None; 100]);
    assert!(result.rows.is_empty());
    assert!(result.truncated);
    result.truncated = false;
    budget.push(&mut result, vec![Some(String::new()); 100]);
    assert!(result.rows.is_empty());
    assert!(result.truncated);
}

#[test]
fn mysql_read_only_cannot_escape_using_procedures_or_implicit_commit_commands() {
    for sql in [
        "CALL change_data()",
        "CREATE TABLE items(id INT)",
        "EXECUTE write_statement",
        "LOCK TABLES items WRITE",
        "COMMIT",
    ] {
        assert!(script::check_read_only_session(sql, Engine::MySql).is_err());
    }
    assert!(
        script::check_read_only_session("WITH c AS (SELECT 1) SELECT * FROM c", Engine::MySql)
            .is_ok()
    );
    assert!(script::check_read_only_session("INSERT INTO items VALUES(1)", Engine::MySql).is_ok());
}

#[test]
fn existing_embedded_files_open_read_only_without_mutation() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("relix-test-{}-{nonce}.db", std::process::id()));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _cleanup = Cleanup(path.clone());
        let config = ConnectionConfig {
            engine,
            database: path.to_string_lossy().into_owned(),
            read_only: false,
            ..Default::default()
        };
        {
            let writable = DatabaseSession::connect(&config).unwrap();
            run(
                &writable,
                "CREATE TABLE items(id INTEGER); INSERT INTO items VALUES(7)",
            );
        }
        let original = std::fs::read(&path).unwrap();
        {
            let read_only = DatabaseSession::connect(&ConnectionConfig {
                read_only: true,
                ..config
            })
            .unwrap();
            assert_eq!(
                run(&read_only, "SELECT id FROM items").results[0].rows[0][0].as_deref(),
                Some("7")
            );
            assert!(
                read_only
                    .execute("INSERT INTO items VALUES(8)", 10, &CancelToken::new())
                    .is_err()
            );
            let objects = read_only.catalog().unwrap();
            assert_eq!(objects.len(), 1);
            assert_eq!(
                read_only.table_details(&objects[0]).unwrap().columns[0].name,
                "id"
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
    }
}

#[test]
fn sqlite_ddl_does_not_report_previous_insert_count() {
    let session = memory(Engine::SQLite);
    run(
        &session,
        "CREATE TABLE items(id INTEGER); INSERT INTO items VALUES(1)",
    );
    let output = run(&session, "CREATE VIEW item_view AS SELECT * FROM items");
    assert_eq!(output.results[0].affected_rows, 0);
}

#[test]
fn sqlite_returning_limit_does_not_discard_writes() {
    let session = memory(Engine::SQLite);
    run(&session, "CREATE TABLE items(id INTEGER)");
    let output = session
        .execute(
            "INSERT INTO items VALUES(1),(2),(3) RETURNING id",
            1,
            &CancelToken::new(),
        )
        .unwrap();
    assert_eq!(output.results[0].rows.len(), 1);
    assert_eq!(output.results[0].affected_rows, 3);
    assert!(output.results[0].truncated);
    assert_eq!(
        run(&session, "SELECT COUNT(*) FROM items").results[0].rows[0][0].as_deref(),
        Some("3")
    );
}

#[test]
fn quoted_catalog_names_remain_data_in_metadata_queries() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let session = memory(engine);
        let name = "odd'\\\"; DROP TABLE preserved; --";
        let quoted = engine.quote_identifier(name);
        run(
            &session,
            &format!(
                "CREATE TABLE {quoted}(id INTEGER PRIMARY KEY); CREATE TABLE preserved(id INTEGER)"
            ),
        );
        let object = session
            .catalog()
            .unwrap()
            .into_iter()
            .find(|object| object.name == name)
            .unwrap();
        assert_eq!(session.describe(&object).unwrap()[0].name, "id");
        assert!(
            session
                .catalog()
                .unwrap()
                .iter()
                .any(|object| object.name == "preserved")
        );
    }
}

#[test]
fn a_pre_cancelled_command_has_no_effect() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let session = memory(engine);
        let cancel = CancelToken::new();
        cancel.cancel();
        assert!(
            session
                .execute("CREATE TABLE forbidden(id INTEGER)", 10, &cancel)
                .is_err()
        );
        assert!(session.catalog().unwrap().is_empty());
    }
}

#[test]
fn embedded_schema_details_preserve_composite_keys_and_unique_indexes() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let session = memory(engine);
        run(
            &session,
            "CREATE TABLE parent(a INTEGER, b INTEGER, PRIMARY KEY(a,b)); CREATE TABLE child(x INTEGER, y INTEGER, label VARCHAR, FOREIGN KEY(x,y) REFERENCES parent(a,b)); CREATE UNIQUE INDEX child_label ON child(label)",
        );
        let object = session
            .catalog()
            .unwrap()
            .into_iter()
            .find(|object| object.name == "child")
            .unwrap();
        let details = session.table_details(&object).unwrap();
        assert_eq!(details.columns.len(), 3);
        let index = details
            .indexes
            .iter()
            .find(|index| index.name == "child_label")
            .unwrap();
        assert!(index.unique);
        assert!(!index.primary_key);
        assert!(index.columns.iter().any(|key| key.contains("label")));
        assert_eq!(details.foreign_keys.len(), 1);
        let key = &details.foreign_keys[0];
        assert_eq!(key.columns, ["x", "y"]);
        assert_eq!(key.referenced_columns, ["a", "b"]);
        assert_eq!(key.referenced_table, "parent");
        let parent = session
            .catalog()
            .unwrap()
            .into_iter()
            .find(|object| object.name == "parent")
            .unwrap();
        let details = session.table_details(&parent).unwrap();
        let primary = details
            .indexes
            .iter()
            .find(|index| index.primary_key)
            .unwrap();
        assert_eq!(primary.columns, ["a", "b"]);
        assert!(primary.unique);
    }
}

#[test]
fn schema_inspection_treats_quoted_names_as_data_and_rejects_nul() {
    for engine in [Engine::SQLite, Engine::DuckDB] {
        let session = memory(engine);
        let name = "odd'\\\"; DROP TABLE preserved; --";
        run(
            &session,
            &format!(
                "CREATE TABLE {}(id INTEGER PRIMARY KEY); CREATE TABLE preserved(id INTEGER)",
                engine.quote_identifier(name)
            ),
        );
        let object = session
            .catalog()
            .unwrap()
            .into_iter()
            .find(|object| object.name == name)
            .unwrap();
        assert_eq!(
            session.table_details(&object).unwrap().columns[0].name,
            "id"
        );
        assert!(
            session
                .catalog()
                .unwrap()
                .iter()
                .any(|object| object.name == "preserved")
        );
        assert!(
            session
                .table_details(&DbObject {
                    name: "invalid\0name".into(),
                    ..object
                })
                .is_err()
        );
    }
}

#[test]
fn sqlite_details_include_rowid_primary_keys_and_foreign_key_actions() {
    let session = memory(Engine::SQLite);
    run(
        &session,
        "CREATE TABLE parent(id INTEGER PRIMARY KEY); CREATE TABLE child(parent_id INTEGER REFERENCES parent ON UPDATE CASCADE ON DELETE SET NULL); CREATE INDEX expression_idx ON child((parent_id + 1))",
    );
    let objects = session.catalog().unwrap();
    let parent = objects
        .iter()
        .find(|object| object.name == "parent")
        .unwrap();
    assert_eq!(
        session.table_details(parent).unwrap().indexes[0].columns,
        ["id"]
    );
    let child = objects
        .iter()
        .find(|object| object.name == "child")
        .unwrap();
    let details = session.table_details(child).unwrap();
    assert_eq!(details.indexes[0].columns, ["(expression)"]);
    assert!(
        details.indexes[0]
            .definition
            .as_deref()
            .unwrap()
            .contains("parent_id + 1")
    );
    assert_eq!(
        details.foreign_keys[0].referenced_columns,
        ["(primary key)"]
    );
    assert_eq!(details.foreign_keys[0].on_update, "CASCADE");
    assert_eq!(details.foreign_keys[0].on_delete, "SET NULL");
}

#[test]
fn oversized_embedded_cells_are_omitted_before_display_allocation() {
    for (engine, sql) in [
        (Engine::SQLite, "SELECT zeroblob(9000000)"),
        (Engine::DuckDB, "SELECT repeat('x', 20000000)"),
    ] {
        let output = memory(engine)
            .execute(sql, 100, &CancelToken::new())
            .unwrap();
        assert!(output.results[0].rows.is_empty());
        assert!(output.results[0].truncated);
    }
}
