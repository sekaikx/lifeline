//! CSV interchange: receive stock from a spreadsheet or another system, and
//! export the stock list and the ledger.

use crate::{days, Action, Inventory, Item, Location, LocationKind, SupplyClass, TxDraft};
use serde::Serialize;

/// What an import did, row by row. Row numbers count the header as row 1.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImportReport {
    /// Data rows read
    pub rows: usize,
    /// Rows that were received into stock
    pub received: usize,
    /// Catalogue items created from the file
    pub new_items: Vec<String>,
    /// Locations created from the file
    pub new_locations: Vec<String>,
    /// One message per row that was not received
    pub errors: Vec<String>,
}

/// Receive stock from a CSV export of another system or a spreadsheet.
///
/// The first row is the header. Rows that name an item not yet in the
/// catalogue create it, which needs a unit weight. Rows without a location
/// use `default_location`, which is created if it does not exist. Each
/// accepted row is a receipt. Problems are reported per row and never stop
/// the import, so only a file with no usable header or no data rows fails.
pub fn import_stock(
    inv: &mut Inventory,
    text: &str,
    user: &str,
    default_location: &str,
) -> Result<ImportReport, String> {
    let rows = parse(text);
    let Some((header, data)) = rows.split_first() else {
        return Err("the file is empty".into());
    };
    let id_col = column(
        header,
        &[
            "sku",
            "item",
            "item_id",
            "code",
            "stock_number",
            "nsn",
            "product_code",
        ],
    )
    .ok_or(
        "no item column: name one of sku, item, item_id, code, stock_number, nsn or product_code",
    )?;
    let qty_col = column(
        header,
        &[
            "qty",
            "quantity",
            "on_hand",
            "quantity_on_hand",
            "count",
            "available",
        ],
    )
    .ok_or("no quantity column: name one of qty, quantity, on_hand or count")?;
    if data.is_empty() {
        return Err("the file has a header but no data rows".into());
    }
    let name_col = column(header, &["name", "description", "item_name", "product"]);
    let class_col = column(header, &["class", "supply_class", "category"]);
    let unit_col = column(header, &["unit", "uom", "unit_of_issue"]);
    let unit_kg_col = column(header, &["unit_kg", "weight_kg", "unit_weight", "weight"]);
    let lot_col = column(header, &["lot", "lot_number", "batch", "batch_number"]);
    let expiry_col = column(
        header,
        &[
            "expiry",
            "expiry_date",
            "expiration",
            "expiration_date",
            "exp_date",
            "best_before",
        ],
    );
    let location_col = column(header, &["location", "bin", "warehouse", "site"]);
    let default_location = default_location.trim();

    let mut report = ImportReport::default();
    for (k, row) in data.iter().enumerate() {
        let n = k + 2;
        report.rows += 1;

        let id = cell(row, Some(id_col));
        if id.is_empty() {
            report.errors.push(format!("row {n}: no item id"));
            continue;
        }
        let Some(qty) = cell(row, Some(qty_col))
            .parse::<f64>()
            .ok()
            .filter(|q| *q > 0.0 && q.is_finite())
        else {
            report.errors.push(format!("row {n}: no quantity"));
            continue;
        };
        let expiry = match cell(row, expiry_col) {
            "" => None,
            e => match iso_date(e) {
                Some(d) => Some(d),
                None => {
                    report
                        .errors
                        .push(format!("row {n}: expiry {e:?} is not a date"));
                    continue;
                }
            },
        };
        let location = match cell(row, location_col) {
            "" => default_location,
            l => l,
        }
        .to_string();
        if location.is_empty() {
            report
                .errors
                .push(format!("row {n}: no location and no default location"));
            continue;
        }

        if inv.item(id).is_none() {
            let unit_kg = cell(row, unit_kg_col)
                .parse::<f64>()
                .ok()
                .filter(|w| *w > 0.0 && w.is_finite());
            let Some(unit_kg) = unit_kg else {
                report
                    .errors
                    .push(format!("row {n}: new item {id} needs a unit weight"));
                continue;
            };
            let item = Item {
                id: id.to_string(),
                name: match cell(row, name_col) {
                    "" => id,
                    s => s,
                }
                .to_string(),
                class: SupplyClass::parse(cell(row, class_col)).unwrap_or_default(),
                unit: match cell(row, unit_col) {
                    "" => "unit",
                    s => s,
                }
                .to_string(),
                unit_kg,
                unit_m3: None,
                cold_chain: None,
                max_transit_h: None,
                hazmat: None,
                controlled: false,
                reorder_level: 0.0,
                notes: String::new(),
            };
            if let Err(e) = inv.upsert_item(item) {
                report.errors.push(format!("row {n}: {e}"));
                continue;
            }
            report.new_items.push(id.to_string());
        }

        if inv.location(&location).is_none() {
            let loc = Location {
                id: location.clone(),
                name: location.clone(),
                kind: LocationKind::Warehouse,
            };
            if let Err(e) = inv.upsert_location(loc) {
                report.errors.push(format!("row {n}: {e}"));
                continue;
            }
            report.new_locations.push(location.clone());
        }

        let draft = TxDraft {
            action: Action::Receive,
            item: id.to_string(),
            lot: cell(row, lot_col).to_string(),
            expiry,
            from: None,
            to: Some(location),
            qty,
            reference: Some("CSV import".into()),
            user: user.into(),
            witness: None,
            note: String::new(),
        };
        match inv.apply(draft) {
            Ok(_) => report.received += 1,
            Err(e) => report.errors.push(format!("row {n}: {e}")),
        }
    }
    Ok(report)
}

/// Current stock, one line per item, lot and location.
pub fn export_stock(inv: &Inventory) -> String {
    let mut out =
        String::from("item,name,class,lot,expiry,location,available,held,unit,unit_kg,total_kg\n");
    for l in inv.stock() {
        let item = inv.item(&l.item);
        let name = item.map_or("", |i| i.name.as_str());
        let class = item.map_or_else(String::new, |i| serde_str(i.class).to_uppercase());
        let unit = item.map_or("", |i| i.unit.as_str());
        let unit_kg = item.map_or(0.0, |i| i.unit_kg);
        let total_kg = (l.available + l.held) * unit_kg;
        let cells = [
            field(&l.item),
            field(name),
            field(&class),
            field(&l.lot),
            field(l.expiry.as_deref().unwrap_or("")),
            field(&l.location),
            l.available.to_string(),
            l.held.to_string(),
            field(unit),
            unit_kg.to_string(),
            total_kg.to_string(),
        ];
        out.push_str(&cells.join(","));
        out.push('\n');
    }
    out
}

/// The whole ledger, one line per entry, oldest first.
pub fn export_ledger(inv: &Inventory) -> String {
    let mut out = String::from(
        "seq,time_utc,action,item,lot,expiry,from,to,qty,reference,user,witness,note,hash\n",
    );
    for t in &inv.ledger {
        let cells = [
            t.seq.to_string(),
            iso_time(t.ts),
            serde_str(t.action),
            t.item.clone(),
            t.lot.clone(),
            t.expiry.clone().unwrap_or_default(),
            t.from.clone().unwrap_or_default(),
            t.to.clone().unwrap_or_default(),
            t.qty.to_string(),
            t.reference.clone().unwrap_or_default(),
            t.user.clone(),
            t.witness.clone().unwrap_or_default(),
            t.note.clone(),
            t.hash.clone(),
        ];
        let line: Vec<String> = cells.iter().map(|c| field(c)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

/// Split CSV text into rows of fields. Blank rows are skipped, so row
/// numbers count the rows that remain. Double quotes protect commas, line
/// breaks and quotes (a quote inside is doubled). A `;` delimiter is used
/// instead of `,` when the first line has more semicolons than commas, as
/// European spreadsheets write it.
fn parse(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let delim = if first.matches(';').count() > first.matches(',').count() {
        ';'
    } else {
        ','
    };

    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c != '"' {
                cell.push(c);
            } else if chars.peek() == Some(&'"') {
                chars.next();
                cell.push('"');
            } else {
                quoted = false;
            }
            continue;
        }
        match c {
            '"' if cell.is_empty() => quoted = true,
            '\r' => {}
            '\n' => end_row(&mut rows, &mut row, &mut cell),
            c if c == delim => row.push(std::mem::take(&mut cell)),
            c => cell.push(c),
        }
    }
    end_row(&mut rows, &mut row, &mut cell);
    rows
}

/// Close the current field and row; keep the row unless it is blank.
fn end_row(rows: &mut Vec<Vec<String>>, row: &mut Vec<String>, cell: &mut String) {
    row.push(std::mem::take(cell));
    if row.iter().any(|f| !f.trim().is_empty()) {
        rows.push(std::mem::take(row));
    } else {
        row.clear();
    }
}

/// Quote a value for CSV output when it contains a delimiter, a quote or a
/// line break.
fn field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Normalise a header: `" Unit kg"` and `"unit-kg"` both become `unit_kg`.
fn norm(h: &str) -> String {
    h.trim().to_lowercase().replace([' ', '-'], "_")
}

/// Index of the first header column whose name is one of `names`.
fn column(header: &[String], names: &[&str]) -> Option<usize> {
    header
        .iter()
        .position(|h| names.contains(&norm(h).as_str()))
}

/// Trimmed contents of a column, or `""` when the file has no such column
/// or the row is short.
fn cell(row: &[String], col: Option<usize>) -> &str {
    col.and_then(|c| row.get(c)).map_or("", |s| s.trim())
}

/// Accept `YYYY-MM-DD`, `YYYY/MM/DD`, `DD/MM/YYYY` and `DD.MM.YYYY` and
/// return `YYYY-MM-DD`, or `None` if the date does not exist.
fn iso_date(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.trim().split(['-', '/', '.']).collect();
    if parts.len() != 3 {
        return None;
    }
    let (y, m, d) = if parts[0].len() == 4 {
        (parts[0], parts[1], parts[2])
    } else if parts[2].len() == 4 {
        (parts[2], parts[1], parts[0])
    } else {
        return None;
    };
    let y: i64 = y.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    let d: u32 = d.parse().ok()?;
    if !(1..=12).contains(&m) || d == 0 || d > month_len(y, m) {
        return None;
    }
    let iso = format!("{y:04}-{m:02}-{d:02}");
    days(&iso).map(|_| iso)
}

fn month_len(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The serde name of a unit-like enum value, e.g. `receive` or `viii`.
fn serde_str<T: Serialize>(v: T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
fn iso_time(ts: i64) -> String {
    let secs = ts.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(ts.div_euclid(86_400));
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's
/// civil_from_days, the inverse of [`days`].
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, controlled: bool) -> Item {
        Item {
            id: id.into(),
            name: format!("{id} item"),
            class: SupplyClass::I,
            unit: "unit".into(),
            unit_kg: 2.0,
            unit_m3: Some(0.01),
            cold_chain: None,
            max_transit_h: None,
            hazmat: None,
            controlled,
            reorder_level: 0.0,
            notes: String::new(),
        }
    }

    fn loc(id: &str) -> Location {
        Location {
            id: id.into(),
            name: id.into(),
            kind: LocationKind::Depot,
        }
    }

    fn catalogue() -> Inventory {
        let mut inv = Inventory::default();
        inv.upsert_item(item("WTR-20", false)).unwrap();
        inv.upsert_item(item("MOR", true)).unwrap();
        inv.upsert_location(loc("Depot North")).unwrap();
        inv
    }

    #[test]
    fn quoted_fields_keep_commas_newlines_and_quotes() {
        let rows = parse("a,\"b,c\",\"d\"\"e\"\r\n\r\n\"multi\nline\",x\n\n");
        assert_eq!(
            rows,
            vec![
                vec!["a".to_string(), "b,c".into(), "d\"e".into()],
                vec!["multi\nline".to_string(), "x".into()],
            ]
        );
    }

    #[test]
    fn bom_and_semicolons() {
        let rows = parse("\u{feff}sku;qty\r\nA;\"1,5\"\n");
        assert_eq!(rows, vec![vec!["sku", "qty"], vec!["A", "1,5"]]);
        // a comma header stays comma separated even when values hold semicolons
        let rows = parse("sku,qty\nA,\"x;y\"\n");
        assert_eq!(rows[1], vec!["A", "x;y"]);
    }

    #[test]
    fn field_quotes_only_when_needed() {
        assert_eq!(field("plain"), "plain");
        assert_eq!(field("a,b"), "\"a,b\"");
        assert_eq!(field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(field("two\nlines"), "\"two\nlines\"");
    }

    #[test]
    fn dates_normalise() {
        assert_eq!(iso_date("2027-01-31").as_deref(), Some("2027-01-31"));
        assert_eq!(iso_date("31/12/2026").as_deref(), Some("2026-12-31"));
        assert_eq!(iso_date("2027/1/5").as_deref(), Some("2027-01-05"));
        assert_eq!(iso_date("29.02.2028").as_deref(), Some("2028-02-29"));
        assert_eq!(iso_date("31.02.2027"), None);
        assert_eq!(iso_date("29.02.2027"), None);
        assert_eq!(iso_date("next year"), None);
    }

    #[test]
    fn import_creates_items_and_locations_and_reports_bad_rows() {
        let mut inv = Inventory::default();
        let text = "SKU,Name,Class,Unit,Unit kg,Lot,Expiry,Qty,Location\n\
            WTR-20,\"Water, 20 L\",I,jerrycan,20,L1,2027-01-31,10,Depot North\n\
            WTR-20,,,,,L2,31/12/2026,5,Depot North\n\
            WTR-20,,,,,L3,31.02.2027,4,Depot North\n\
            WTR-20,,,,,L4,,0,Depot North\n\
            KIT-1,Hygiene kit,II,kit,,,,3,\n";
        let report = import_stock(&mut inv, text, "Sgt Ito", "Depot North").unwrap();
        assert_eq!(report.rows, 5);
        assert_eq!(report.received, 2);
        assert_eq!(report.new_items, vec!["WTR-20"]);
        assert_eq!(report.new_locations, vec!["Depot North"]);
        assert_eq!(report.errors.len(), 3, "{:?}", report.errors);
        assert!(report.errors[0].starts_with("row 4:"));
        assert_eq!(report.errors[1], "row 5: no quantity");
        assert!(report.errors[2].contains("row 6") && report.errors[2].contains("unit weight"));
        assert_eq!(inv.available("WTR-20", Some("Depot North")), 15.0);
        assert_eq!(inv.item("WTR-20").unwrap().name, "Water, 20 L");
        assert_eq!(inv.ledger[1].expiry.as_deref(), Some("2026-12-31"));
        assert!(inv.item("KIT-1").is_none());
    }

    #[test]
    fn import_needs_an_item_column_a_quantity_column_and_rows() {
        let mut inv = Inventory::default();
        assert!(import_stock(&mut inv, "name,qty\nA,1\n", "u", "A").is_err());
        assert!(import_stock(&mut inv, "sku,name\nA,x\n", "u", "A").is_err());
        assert!(import_stock(&mut inv, "sku,qty\n", "u", "A").is_err());
        assert!(import_stock(&mut inv, "", "u", "A").is_err());
    }

    #[test]
    fn controlled_items_report_a_witness_error_per_row() {
        let mut inv = catalogue();
        let text = "sku,qty,location\nMOR,3,Depot North\nWTR-20,2,Depot North\nMOR,1,Depot North\n";
        let report = import_stock(&mut inv, text, "Sgt Ito", "Depot North").unwrap();
        assert_eq!(report.received, 1);
        assert_eq!(report.errors.len(), 2);
        assert!(report.errors.iter().all(|e| e.contains("witness")));
        assert!(report.errors[0].starts_with("row 2:"));
        assert_eq!(inv.available("MOR", None), 0.0);
    }

    #[test]
    fn exported_stock_imports_back_with_the_same_totals() {
        let mut first = catalogue();
        first
            .upsert_item(Item {
                name: "Hygiene, basic".into(),
                ..item("HYG", false)
            })
            .unwrap();
        let text = "sku,name,qty,lot,expiry,location\n\
            WTR-20,,10,L1,2027-01-31,Depot North\n\
            WTR-20,,5,L2,,Depot North\n\
            HYG,,2,,,Depot North\n";
        let report = import_stock(&mut first, text, "Sgt Ito", "Depot North").unwrap();
        assert_eq!(report.received, 3, "{:?}", report.errors);
        first
            .apply(TxDraft {
                action: Action::Issue,
                item: "WTR-20".into(),
                lot: "L1".into(),
                expiry: None,
                from: Some("Depot North".into()),
                to: None,
                qty: 4.0,
                reference: Some("REQ-7".into()),
                user: "Sgt Ito".into(),
                witness: None,
                note: String::new(),
            })
            .unwrap();

        let export = export_stock(&first);
        assert!(export.starts_with(
            "item,name,class,lot,expiry,location,available,held,unit,unit_kg,total_kg\n"
        ));
        assert!(export.contains("\"Hygiene, basic\""));
        assert!(export.contains("WTR-20,WTR-20 item,I,L1,2027-01-31,Depot North,6,0,unit,2,12\n"));

        let mut second = Inventory::default();
        for i in first.items.clone() {
            second.upsert_item(i).unwrap();
        }
        for l in first.locations.clone() {
            second.upsert_location(l).unwrap();
        }
        let report = import_stock(&mut second, &export, "Sgt Ito", "Depot North").unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.received, 3);
        for id in ["WTR-20", "HYG", "MOR"] {
            assert_eq!(
                second.available(id, None),
                first.available(id, None),
                "{id}"
            );
        }
    }

    #[test]
    fn ledger_export_has_one_line_per_entry_and_iso_times() {
        let mut inv = catalogue();
        inv.apply_at(
            TxDraft {
                action: Action::Receive,
                item: "WTR-20".into(),
                lot: "L1".into(),
                expiry: Some("2027-01-31".into()),
                from: None,
                to: Some("Depot North".into()),
                qty: 10.0,
                reference: None,
                user: "Sgt Ito".into(),
                witness: None,
                note: String::new(),
            },
            0,
        )
        .unwrap();
        inv.apply_at(
            TxDraft {
                action: Action::Issue,
                item: "WTR-20".into(),
                lot: "L1".into(),
                expiry: None,
                from: Some("Depot North".into()),
                to: None,
                qty: 3.0,
                reference: Some("REQ-1, urgent".into()),
                user: "Sgt Ito".into(),
                witness: None,
                note: "said \"first\"".into(),
            },
            1_700_000_000,
        )
        .unwrap();

        let out = export_ledger(&inv);
        assert_eq!(out.lines().count(), inv.ledger.len() + 1);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].starts_with("seq,time_utc,action,item,"));
        assert!(lines[1]
            .starts_with("1,1970-01-01T00:00:00Z,receive,WTR-20,L1,2027-01-31,,Depot North,10,"));
        assert!(lines[2].starts_with(
            "2,2023-11-14T22:13:20Z,issue,WTR-20,L1,2027-01-31,Depot North,,3,\"REQ-1, urgent\","
        ));
        assert!(lines[2].contains("\"said \"\"first\"\"\""));
    }

    #[test]
    fn time_conversion() {
        assert_eq!(iso_time(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_time(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso_time(-1), "1969-12-31T23:59:59Z");
        assert_eq!(iso_time(1_700_000_000), "2023-11-14T22:13:20Z");
    }
}
