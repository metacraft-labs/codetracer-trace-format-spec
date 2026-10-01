// Prints a member of a container, inflating it chunk by chunk when it has a
// companion `.idx`. Usage: dump_member FILE.ct MEMBER [max_bytes]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = ctfs_measure::ctfs::Container::open(std::path::Path::new(&a[1])).unwrap();
    let max: usize = a.get(3).and_then(|s| s.parse().ok()).unwrap_or(256);
    let data = c.read(&a[2]).unwrap();
    let idxname = a[2].replace(".dat", ".idx");
    let raw = match c.read(&idxname) {
        Ok(idx) if idxname != a[2] => {
            let (_, offs) = ctfs_measure::ctfs::parse_idx(&idx).unwrap();
            ctfs_measure::ctfs::frames(&data, &offs).iter().flat_map(|f| ctfs_measure::zst::decompress(f)).collect()
        }
        _ => data,
    };
    println!("{} bytes", raw.len());
    for (i, ch) in raw[..raw.len().min(max)].chunks(32).enumerate() {
        println!("{:6}: {}", i * 32, ch.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
    }
}
