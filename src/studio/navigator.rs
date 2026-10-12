//! Catalog hierarchy derived from the active retained session, without extra I/O.

use crate::database::{ConnectionConfig, DbObject, ObjectKind};
use gpui_kit::component::tree::TreeItem;
use std::collections::{BTreeMap, HashSet};

pub(super) fn catalog_tree(
    config: &ConnectionConfig,
    catalog: &[DbObject],
    filter: &str,
    collapsed: &HashSet<String>,
) -> Vec<TreeItem> {
    let filter = filter.trim().to_lowercase();
    let mut schemas: BTreeMap<&str, Vec<(usize, &DbObject)>> = BTreeMap::new();
    for (index, object) in catalog.iter().enumerate() {
        if format!("{}.{}", object.schema, object.name)
            .to_lowercase()
            .contains(&filter)
        {
            schemas
                .entry(&object.schema)
                .or_default()
                .push((index, object));
        }
    }
    if schemas.is_empty() {
        return Vec::new();
    }
    let branch = |id: String, label: String, children: Vec<TreeItem>| {
        let expanded = !filter.is_empty() || !collapsed.contains(&id);
        TreeItem::new(id, label)
            .expanded(expanded)
            .children(children)
    };
    let schema_nodes = schemas
        .into_iter()
        .map(|(schema, objects)| {
            // Length-prefix the connection identity so unusual schema names cannot
            // collide with nodes owned by another session.
            let key = format!("{}:{}{}", config.id.len(), config.id, schema);
            let groups = [
                (ObjectKind::Table, "Tables", "tables"),
                (ObjectKind::View, "Views", "views"),
            ]
            .into_iter()
            .filter_map(|(kind, label, prefix)| {
                let mut matches = objects
                    .iter()
                    .filter(|(_, object)| object.kind == kind)
                    .copied()
                    .collect::<Vec<_>>();
                matches.sort_by(|(_, a), (_, b)| a.name.cmp(&b.name));
                if matches.is_empty() {
                    return None;
                }
                let children = matches
                    .iter()
                    .map(|(index, object)| {
                        TreeItem::new(format!("object-{index}"), object.name.clone())
                    })
                    .collect::<Vec<_>>();
                Some(branch(
                    format!("{prefix}-{key}"),
                    format!("{label} · {}", children.len()),
                    children,
                ))
            })
            .collect::<Vec<_>>();
            branch(
                format!("schema-{key}"),
                if schema.is_empty() {
                    "Default schema".into()
                } else {
                    schema.to_owned()
                },
                groups,
            )
        })
        .collect::<Vec<_>>();
    let database = if config.database == ":memory:" {
        "In-memory database".to_owned()
    } else if matches!(
        config.engine,
        crate::database::Engine::SQLite | crate::database::Engine::DuckDB
    ) {
        std::path::Path::new(&config.database)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| config.database.clone())
    } else {
        config.database.clone()
    };
    let schemas = branch(
        format!("schemas-{}", config.id),
        "Schemas".into(),
        schema_nodes,
    );
    let database = branch(format!("database-{}", config.id), database, vec![schemas]);
    vec![branch(
        format!("connection-{}", config.id),
        config.name.clone(),
        vec![database],
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objects() -> Vec<DbObject> {
        vec![
            DbObject {
                schema: "public".into(),
                name: "zebra".into(),
                kind: ObjectKind::Table,
            },
            DbObject {
                schema: "analytics".into(),
                name: "summary".into(),
                kind: ObjectKind::View,
            },
            DbObject {
                schema: "public".into(),
                name: "accounts".into(),
                kind: ObjectKind::Table,
            },
        ]
    }

    #[test]
    fn hierarchy_groups_and_sorts_without_changing_catalog_indices() {
        let tree = catalog_tree(
            &ConnectionConfig::default(),
            &objects(),
            "",
            &HashSet::new(),
        );
        let schemas = &tree[0].children[0].children[0].children;
        assert_eq!(
            schemas
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>(),
            ["analytics", "public"]
        );
        assert_eq!(schemas[0].children[0].label.as_str(), "Views · 1");
        let tables = &schemas[1].children[0];
        assert_eq!(tables.label.as_str(), "Tables · 2");
        assert_eq!(tables.children[0].id.as_str(), "object-2");
        assert_eq!(tables.children[1].id.as_str(), "object-0");
        assert_eq!(tables.children[0].label.as_str(), "accounts");
    }

    #[test]
    fn search_reveals_ancestors_without_erasing_collapse_preferences() {
        let config = ConnectionConfig::default();
        let initial = catalog_tree(&config, &objects(), "", &HashSet::new());
        let collapsed = HashSet::from([initial[0].id.to_string()]);
        assert!(!catalog_tree(&config, &objects(), "", &collapsed)[0].is_expanded());
        let search = catalog_tree(&config, &objects(), " ACCOUNT ", &collapsed);
        assert!(search[0].is_expanded());
        let schemas = &search[0].children[0].children[0].children;
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].children[0].children[0].id.as_str(), "object-2");
        assert!(!catalog_tree(&config, &objects(), "", &collapsed)[0].is_expanded());
        assert!(catalog_tree(&config, &objects(), "missing", &collapsed).is_empty());
    }
}
