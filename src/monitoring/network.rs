use sysinfo::Networks;

#[derive(Debug, Default, Clone, Copy)]
pub struct NetworkRates {
    pub received_bytes_per_sec: u64,
    pub transmitted_bytes_per_sec: u64,
}

/// Calculate traffic rates from cumulative interface byte counters.
/// The first call establishes a baseline and returns zero rates.
pub fn sample(networks: &mut Networks, previous: &mut Option<(u64, u64)>) -> NetworkRates {
    networks.refresh();
    let mut received = 0u64;
    let mut transmitted = 0u64;
    for (_name, data) in networks.iter() {
        received = received.saturating_add(data.total_received());
        transmitted = transmitted.saturating_add(data.total_transmitted());
    }
    let rates = previous
        .map(|(old_rx, old_tx)| NetworkRates {
            received_bytes_per_sec: received.saturating_sub(old_rx),
            transmitted_bytes_per_sec: transmitted.saturating_sub(old_tx),
        })
        .unwrap_or_default();
    *previous = Some((received, transmitted));
    rates
}
