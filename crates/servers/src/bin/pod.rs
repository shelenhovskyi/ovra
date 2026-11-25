//! Pod mode alone, for tests: `ovra-pod --ovra-pod <pod dir>`.

fn main() {
    ovra_servers::pod_init_if_asked();
    eprintln!("ovra-pod runs only as a server pod");
    std::process::exit(2);
}
