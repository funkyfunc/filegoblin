fn main() {
    let html3 = "<ul><li>Item 1<ol><li>Sub 1</li></ol><li>Item 2</li></ul>";
    println!("{}", html2md::parse_html(html3));
}
