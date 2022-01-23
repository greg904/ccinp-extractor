use std::convert::TryFrom;
use std::str;

use mupdf::pdf::PdfDocument;
use mupdf::pdf::PdfObject;
use mupdf::pdf::PdfWriteOptions;

enum FilterResult {
    KeepKid,
    RemoveKid,
    SubtractCount(i32),
}

fn filter_page_tree_helper<F: FnMut(PdfObject) -> bool>(mut node: PdfObject, f: &mut F) -> FilterResult {
    let ty = node.get_dict("Type").unwrap().unwrap();
    let ty_str = ty.as_name().unwrap();
    match ty_str {
        b"Page" => if f(node) { FilterResult::KeepKid } else { FilterResult::RemoveKid },
        b"Pages" => {
            let mut subtract_count = 0;
            if let Some(mut kids) = node.get_dict("Kids").unwrap() {
                let mut kids_len = i32::try_from(kids.len().unwrap()).unwrap();
                let mut i: i32 = 0;
                while i < kids_len {
                    let kid = kids.get_array(i).unwrap().unwrap();
                    match filter_page_tree_helper(kid, f) {
                        FilterResult::KeepKid => {},
                        FilterResult::RemoveKid => {
                            kids.array_delete(i).unwrap();
                            kids_len -= 1;
                            subtract_count += 1;
                            continue;
                        },
                        FilterResult::SubtractCount(n) => subtract_count += n,
                    }
                    i += 1;
                }
                if subtract_count > 0 {
                    let count = node.get_dict("Count").unwrap().unwrap()
                        .as_int().unwrap();
                    node.dict_put("Count", PdfObject::new_int(count - subtract_count).unwrap()).unwrap();
                }
            }
            FilterResult::SubtractCount(subtract_count)
        },
        _ => panic!("invalid type in page tree"),
    }
}

fn filter_page_tree<F: FnMut(PdfObject) -> bool>(root: PdfObject, mut f: F) {
    filter_page_tree_helper(root, &mut f);
}

fn main() {
    let doc = PdfDocument::open("exercises.pdf").expect("failed to load PDF");
    let catalog_id = doc.catalog().unwrap();
    let mut catalog = catalog_id.resolve().unwrap().unwrap();
    let tree_id = catalog.get_dict("Pages").unwrap().unwrap();
    let tree = tree_id.resolve().unwrap().unwrap();
    let mut current_exercise_number = -1;
    filter_page_tree(tree, |page: PdfObject| {
        let contents = page.get_dict("Contents").unwrap().unwrap()
            .read_stream().unwrap();
        let contents_str = str::from_utf8(&contents).unwrap();
        let marker = "(EXER)31(CICE)";
        if let Some(i) = contents_str.find(marker) {
            let lparen = (i + marker.len()) + contents_str[i + marker.len()..].find('(').unwrap();
            let rparen = (lparen + 1) + contents_str[(lparen + 1)..].find(')').unwrap();
            current_exercise_number = contents_str[(lparen + 1)..rparen].parse().unwrap();
        }
        // Remove the page unless it's the good exercise.
        current_exercise_number == 105
    });
    catalog.dict_delete("Outlines").unwrap();
    doc.save("exercises-extracted.pdf").expect("failed to save PDF");
}
