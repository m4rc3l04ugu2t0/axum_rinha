use db::Page;

fn main() {
    let mut page = Page::<2042>::new();

    page.insert(String::from("sla1")).unwrap();

    page.rows().next().unwrap();
}
