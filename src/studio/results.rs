use crate::database::QueryResult;
use gpui_kit::component::{
    ActiveTheme,
    table::{Column, ColumnSort, TableDelegate, TableState},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{App, Context, IntoElement, Window, div, prelude::*};

pub struct QueryResults {
    pub rows: Vec<Vec<Option<String>>>,
    all_rows: Vec<Vec<Option<String>>>,
    columns: Vec<Column>,
    types: Vec<String>,
    filter_text: String,
}
impl QueryResults {
    pub fn empty() -> Self {
        Self {
            rows: Vec::new(),
            all_rows: Vec::new(),
            columns: Vec::new(),
            types: Vec::new(),
            filter_text: String::new(),
        }
    }
    pub fn from_result(result: &QueryResult) -> Self {
        Self {
            rows: result.rows.clone(),
            all_rows: result.rows.clone(),
            columns: result
                .columns
                .iter()
                .enumerate()
                .map(|(i, col)| {
                    Column::new(format!("column-{i}"), col.name.clone())
                        .width(160.)
                        .min_width(70.)
                        .sortable()
                })
                .collect(),
            types: result.columns.iter().map(|c| c.data_type.clone()).collect(),
            filter_text: String::new(),
        }
    }
    pub fn filter(&mut self, text: &str) {
        let text = text.to_lowercase();
        self.filter_text = text.clone();
        for column in &mut self.columns {
            column.sort = Some(ColumnSort::Default);
        }
        self.rows = self
            .all_rows
            .iter()
            .filter(|row| {
                text.is_empty()
                    || row.iter().any(|cell| {
                        cell.as_deref()
                            .unwrap_or("NULL")
                            .to_lowercase()
                            .contains(&text)
                    })
            })
            .cloned()
            .collect();
    }
    pub fn headers(&self) -> Vec<String> {
        self.columns.iter().map(|c| c.name.to_string()).collect()
    }
    pub fn loaded_count(&self) -> usize {
        self.all_rows.len()
    }

    pub fn cell_details(
        &self,
        row: usize,
        column: usize,
    ) -> Option<(String, String, Option<String>)> {
        Some((
            self.columns.get(column)?.name.to_string(),
            self.types.get(column)?.clone(),
            self.rows.get(row)?.get(column)?.clone(),
        ))
    }
    fn sort_rows(&mut self, index: usize, sort: ColumnSort) {
        if sort == ColumnSort::Default {
            let filter = self.filter_text.clone();
            self.filter(&filter);
            return;
        }
        let values = self
            .rows
            .iter()
            .filter_map(|row| row.get(index).and_then(Option::as_deref))
            .collect::<Vec<_>>();
        let integers = values.iter().all(|value| value.parse::<i128>().is_ok());
        let numbers = values.iter().all(|value| value.parse::<f64>().is_ok());
        // Pick one ordering for the whole column. Switching between numeric and
        // lexical comparison per pair would violate the sort's transitivity.
        self.rows.sort_by(|a, b| {
            let left = a.get(index).and_then(Option::as_ref);
            let right = b.get(index).and_then(Option::as_ref);
            let order = match (left, right) {
                (Some(left), Some(right)) if integers => {
                    match (left.parse::<i128>(), right.parse::<i128>()) {
                        (Ok(left), Ok(right)) => left.cmp(&right),
                        _ => left.cmp(right),
                    }
                }
                (Some(left), Some(right)) if numbers => {
                    match (left.parse::<f64>(), right.parse::<f64>()) {
                        (Ok(a), Ok(b)) => a.total_cmp(&b).then_with(|| left.cmp(right)),
                        _ => left.cmp(right),
                    }
                }
                _ => left.cmp(&right),
            };
            if sort == ColumnSort::Descending {
                order.reverse()
            } else {
                order
            }
        });
        for column in &mut self.columns {
            column.sort = Some(ColumnSort::Default);
        }
        self.columns[index].sort = Some(sort);
    }
}
impl TableDelegate for QueryResults {
    fn columns_count(&self, _: &App) -> usize {
        self.columns.len()
    }
    fn rows_count(&self, _: &App) -> usize {
        self.rows.len()
    }
    fn column(&self, index: usize, _: &App) -> Column {
        self.columns[index].clone()
    }
    fn perform_sort(
        &mut self,
        index: usize,
        sort: ColumnSort,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) {
        self.sort_rows(index, sort);
        cx.notify();
    }
    fn render_th(
        &mut self,
        index: usize,
        _: &mut Window,
        _: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        let name = self.columns[index].name.clone();
        let description = format!("{} ({})", name, self.types[index]);
        // Kit fixes header height to the row size. Keep metadata available
        // without stacking a second line that would clip in compact tables.
        div()
            .id(("result-column-label", index))
            .min_w_0()
            .truncate()
            .aria_label(description.clone())
            .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
            .child(name)
    }
    fn render_td(
        &mut self,
        row: usize,
        col: usize,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        div()
            .min_w_0()
            .truncate()
            .when(self.rows[row][col].is_none(), |el| {
                el.text_color(cx.theme().muted_foreground).italic()
            })
            .child(self.rows[row][col].clone().unwrap_or("NULL".into()))
    }
    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<TableState<Self>>,
    ) -> impl IntoElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .text_color(cx.theme().muted_foreground)
            .child("No rows to display")
            .child(
                div()
                    .text_xs()
                    .child("Run a query, or clear the loaded-row filter."),
            )
    }
    fn cell_text(&self, row: usize, col: usize, _: &App) -> String {
        self.rows[row][col].clone().unwrap_or("NULL".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::ResultColumn;
    #[test]
    fn loaded_rows_sort_consistently_and_filter_preserves_sql_null() {
        let result = QueryResult {
            columns: vec![ResultColumn {
                name: "value".into(),
                data_type: "TEXT".into(),
            }],
            rows: vec![
                vec![Some("2".into())],
                vec![Some("10".into())],
                vec![Some("1x".into())],
                vec![None],
            ],
            ..Default::default()
        };
        let mut table = QueryResults::from_result(&result);
        table.sort_rows(0, ColumnSort::Ascending);
        assert_eq!(
            table.rows,
            vec![
                vec![None],
                vec![Some("10".into())],
                vec![Some("1x".into())],
                vec![Some("2".into())]
            ]
        );
        table.filter("NULL");
        assert_eq!(table.rows, vec![vec![None]]);
        table.filter("");
        table.sort_rows(0, ColumnSort::Default);
        assert_eq!(table.rows, result.rows);
        let result = QueryResult {
            rows: vec![
                vec![Some("9223372036854775807".into())],
                vec![Some("9223372036854775806".into())],
            ],
            ..result
        };
        let mut table = QueryResults::from_result(&result);
        table.sort_rows(0, ColumnSort::Ascending);
        assert_eq!(table.rows[0][0].as_deref(), Some("9223372036854775806"));
    }

    #[test]
    fn cell_details_keep_raw_text_and_null_after_filtering() {
        let result = QueryResult {
            columns: vec![ResultColumn {
                name: "payload".into(),
                data_type: "TEXT".into(),
            }],
            rows: vec![
                vec![None],
                vec![Some("NULL".into())],
                vec![Some("line 1\nline 2\t".into())],
                vec![Some(String::new())],
            ],
            ..Default::default()
        };
        let mut table = QueryResults::from_result(&result);
        assert_eq!(
            table.cell_details(0, 0),
            Some(("payload".into(), "TEXT".into(), None))
        );
        assert_eq!(
            table.cell_details(1, 0).map(|details| details.2),
            Some(Some("NULL".into()))
        );
        assert_eq!(
            table.cell_details(3, 0).map(|details| details.2),
            Some(Some(String::new()))
        );
        table.filter("line 2");
        assert_eq!(table.loaded_count(), 4);
        assert_eq!(
            table.cell_details(0, 0).map(|details| details.2),
            Some(Some("line 1\nline 2\t".into()))
        );
        assert_eq!(table.cell_details(1, 0), None);
        assert_eq!(table.cell_details(0, 1), None);
    }
}
