use std::str;

use pdf::build::{CatalogBuilder, PageBuilder};
use pdf::content::{Op, TextDrawAdjusted};
use pdf::file::File;
use pdf::primitive::Primitive;

fn read_exercise_number_from_op(op: &Op) -> Option<i32> {
    let text = match op {
        Op::TextDrawAdjusted { array } => array,
        _ => return None,
    };
    if text.len() < 5 {
        return None;
    }
    match &text[0] {
        TextDrawAdjusted::Text(s) if s.as_ref() == b"EXER" => {},
        _ => return None,
    };
    match &text[2] {
        TextDrawAdjusted::Text(s) if s.as_ref() == b"CICE" => {},
        _ => return None,
    };
    match &text[4] {
        TextDrawAdjusted::Text(s) => s.to_string().ok()?.parse().ok(),
        _ => None,
    }
}

fn main() {
    let mut file = File::open("exercises.pdf").expect("failed to load PDF");
    let mut current_exercise_number = -1;
    let mut pages = vec![];
    for page in file.pages() {
        let page = page.expect("failed to decode page");
        if let Some(contents) = &page.contents {
            let ops = contents.operations(&file).unwrap();
            match ops.iter().find_map(read_exercise_number_from_op) {
                Some(val) => current_exercise_number = val,
                None => {},
            }
        }
        if current_exercise_number == 105 {
            pages.push(PageBuilder::from_page(&page).unwrap());
        }
    }
    let catalog = CatalogBuilder::from_pages(pages)
        .build(&mut file).unwrap();
    file.update_catalog(catalog).expect("failed to update catalog");
    file.save_to("exercises-extracted.pdf").expect("failed to save PDF");
}
