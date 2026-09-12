//! Launch-compiled catalog binary. Hosts compile this package from `CatalogDir`.

use tinker_catalog::entries;

fn main() {
    for entry in entries() {
        println!("{}", entry.id());
    }
}
