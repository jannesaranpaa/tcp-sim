use rand::Rng;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
enum Packet {
    Ack { ack_seq: u64 },
    Data { seq: u64, payload: Vec<u8> },
}

impl Packet {
    fn new_ack(ack_seq: u64) -> Self {
        Packet::Ack { ack_seq }
    }

    fn new_data(seq: u64, payload: Vec<u8>) -> Self {
        Packet::Data { seq, payload }
    }

    fn rand_data(seq: u64, size: usize) -> Self {
        let mut data = vec![0u8; size];
        rand::rng().fill_bytes(&mut data);

        Packet::new_data(seq, data)
    }
}

type Address = usize;

struct Link {
    channels: HashMap<Address, VecDeque<Packet>>,
    capacity: usize,
    last_addr: Address,
}

impl Link {
    fn new() -> Self {
        Self {
            channels: HashMap::new(),
            capacity: 3,
            last_addr: 0,
        }
    }

    fn register(&mut self) -> Address {
        let addr = self.last_addr;
        self.channels.insert(addr, VecDeque::new());
        self.last_addr += 1;

        addr
    }

    fn send(&mut self, packet: Packet, addr: Address) {
        if let Some(channel) = self.channels.get_mut(&addr) {
            if channel.len() < self.capacity {
                channel.push_back(packet);
            }
        }
    }

    fn receive(&mut self, addr: Address) -> Option<Packet> {
        if let Some(channel) = self.channels.get_mut(&addr) {
            return channel.pop_front();
        }

        None
    }
}

struct TCP {
    name: String,
    address: Address,

    unacknowledged: Vec<u64>,

    // Standard TCP state
    smss: u32,
    rwnd: u32,
    cwnd: u32,
    ssthresh: u32,

    // NewReno
    recover: u64,
    in_fast_recovery: bool,
    dup_acks: u32,

    // RFC 3042 Option
    limited_transmit: bool,
}

impl TCP {
    fn flight_size(&self) -> u32 {
        self.unacknowledged.len() as u32
    }

    fn send_window(&self) -> u32 {
        self.cwnd.min(self.rwnd)
    }

    fn new(name: &str, link: &mut Link) -> Self {
        Self {
            name: String::from(name),

            smss: 1, // One packet!

            rwnd: 10,
            cwnd: 10,

            ssthresh: 64,

            unacknowledged: vec![],
            address: link.register(),

            recover: 0,
            dup_acks: 0,
            in_fast_recovery: false,

            limited_transmit: false,
        }
    }

    fn with_limited_transmit(mut self, enabled: bool) -> Self {
        self.limited_transmit = enabled;
        self
    }

    fn send(&mut self, data: Packet, to: Address, link: &mut Link) {
        match data {
            Packet::Data { seq, .. } => self.unacknowledged.push(seq),
            _ => (),
        }

        link.send(data, to);
    }

    fn receive(&mut self, link: &mut Link, peer_addr: Address) -> Option<Packet> {
        let data = link.receive(self.address)?;
        println!("{} received: {:?}", self.name, data);

        if let Packet::Ack { ack_seq } = data {
            self.on_ack_received(ack_seq, peer_addr, link);
        }

        Some(data)
    }

    fn on_ack_received(&mut self, ack_seq: u64, peer_addr: Address, link: &mut Link) {
        let newly_acked = self
            .unacknowledged
            .iter()
            .filter(|&&seq| seq <= ack_seq)
            .count();

        if newly_acked > 0 {
            self.unacknowledged.retain(|&seq| seq > ack_seq);

            if self.in_fast_recovery {
                if ack_seq >= self.recover {
                    // FULL ACK: Exits Fast Recovery
                    println!(
                        "{} -> Full ACK ({}) received! Exiting Fast Recovery.",
                        self.name, ack_seq
                    );
                    self.cwnd = self.ssthresh; // Deflate window back to ssthresh
                    self.dup_acks = 0;
                    self.in_fast_recovery = false;
                } else {
                    // PARTIAL ACK: Retransmit the next unacknowledged packet immediately
                    println!(
                        "{} -> Partial ACK ({}) received! Retransmitting next missing packet.",
                        self.name, ack_seq
                    );

                    // Deflate cwnd by amount of new data, plus 1 SMSS per NewReno spec
                    self.cwnd = self.cwnd.saturating_sub(newly_acked as u32) + self.smss;

                    // Immediately retransmit the first missing packet
                    if let Some(&missing_seq) = self.unacknowledged.first() {
                        println!(
                            "{} -> [Partial ACK Retransmit] Sending seq {}",
                            self.name, missing_seq
                        );
                        link.send(Packet::rand_data(missing_seq, 20), peer_addr);
                    }
                }
            } else {
                self.dup_acks = 0;
                if self.cwnd < self.ssthresh {
                    // Slow Start: Exponential growth (+1 SMSS per ACK)
                    self.cwnd += self.smss;
                } else {
                    // Congestion Avoidance: Linear growth (+1/cwnd per ACK)
                    self.cwnd += self.smss / self.cwnd.max(1);
                }
            }
        } else {
            // --- DUPLICATE ACK ---
            self.dup_acks += 1;

            if !self.in_fast_recovery {
                if self.dup_acks == 3 {
                    // TRIGGER FAST RETRANSMISSION & ENTER FAST RECOVERY
                    println!(
                        "⚡ {} -> 3x DupACK ({})! Triggering Fast Retransmit.",
                        self.name, ack_seq
                    );

                    // 1. Set recover to the highest sequence number sent so far
                    self.recover = *self.unacknowledged.last().unwrap_or(&ack_seq);

                    // 2. Adjust ssthresh = max(FlightSize / 2, 2 * SMSS)
                    let flight_size = self.flight_size();
                    self.ssthresh = (flight_size / 2).max(2 * self.smss);

                    // 3. Inflate cwnd = ssthresh + 3 * SMSS
                    self.cwnd = self.ssthresh + 3 * self.smss;
                    self.in_fast_recovery = true;

                    // 4. Retransmit the missing packet (first unacknowledged)
                    if let Some(&missing_seq) = self.unacknowledged.first() {
                        println!(
                            "{} -> [Fast Retransmit] Resending lost packet seq {}",
                            self.name, missing_seq
                        );
                        link.send(Packet::rand_data(missing_seq, 20), peer_addr);
                    }
                } else if self.limited_transmit && self.dup_acks < 3 {
                    // --- RFC 3042: LIMITED TRANSMIT ---
                    // Transmit 1 unsent packet for each of the 1st and 2nd duplicate ACKs
                    let next_unsent_seq = self.unacknowledged.last().map_or(0, |last| last + 1);

                    if self.flight_size() < self.send_window() {
                        println!(
                            "{} -> [Limited Transmit] DupACK #{} received! Transmitting new seq {}",
                            self.name, self.dup_acks, next_unsent_seq
                        );
                        self.send(Packet::rand_data(next_unsent_seq, 20), peer_addr, link);
                    }
                }
            } else {
                // Additional DupACK while in Fast Recovery: Inflate cwnd by 1 SMSS
                self.cwnd += self.smss;
            }
        }
    }

    fn print(&self) {
        let phase = if self.in_fast_recovery {
            "FAST RECOVERY"
        } else if self.cwnd < self.ssthresh {
            "SLOW START"
        } else {
            "CONGESTION AVOIDANCE"
        };

        let unack_str = format!("{:?}", self.unacknowledged);

        println!("┌──────────────────────────────────────────────┐");
        println!("│ {:<44} │", format!("Endpoint: {} [{}]", self.name, phase));
        println!("├──────────────────────────────────────────────┤");
        println!("│ Address:          {:<26} │", self.address);
        println!("│ cwnd:             {:<26} │", self.cwnd);
        println!("│ ssthresh:         {:<26} │", self.ssthresh);
        println!("│ dup_acks:         {:<26} │", self.dup_acks);
        println!("│ limited_transmit: {:<26} │", self.limited_transmit);
        println!("│ recover:          {:<26} │", self.recover);
        println!("│ FlightSize:       {:<26} │", self.flight_size());
        println!("│ unacknowledged:   {:<26} │", unack_str);
        println!("└──────────────────────────────────────────────┘\n");
    }
}

fn drain_channel(endpoint: &mut TCP, link: &mut Link, peer_addr: Address) -> Vec<Packet> {
    let mut received = Vec::new();
    while let Some(packet) = endpoint.receive(link, peer_addr) {
        received.push(packet);
    }
    received
}

fn receive_and_ack_with_dups(
    receiver: &mut TCP,
    sender_addr: Address,
    link: &mut Link,
    expected_seq: &mut u64,
) {
    while let Some(packet) = receiver.receive(link, sender_addr) {
        if let Packet::Data { seq, .. } = packet {
            if seq == *expected_seq {
                // In-order packet received: advance expected sequence
                *expected_seq += 1;
                receiver.send(Packet::new_ack(seq), sender_addr, link);
            } else {
                // Out-of-order packet! Repeat ACK for the last successfully received sequence
                let last_ack = if *expected_seq > 0 {
                    *expected_seq - 1
                } else {
                    0
                };
                println!(
                    "--> Out-of-order seq {}! Sending duplicate ACK for {}",
                    seq, last_ack
                );
                receiver.send(Packet::new_ack(last_ack), sender_addr, link);
            }
        }
    }
}

fn test_limited_transmit() {
    println!("\n==========================================");
    println!("  TEST: RFC 3042 Limited Transmit");
    println!("==========================================\n");

    let mut link = Link::new();
    link.capacity = 10;

    let mut alice = TCP::new("Alice", &mut link).with_limited_transmit(true);
    let mut bob = TCP::new("Bob", &mut link);

    // Initial transmission batch (seq 0, 1, 2)
    for seq in 0..3 {
        alice.send(Packet::rand_data(seq, 20), bob.address, &mut link);
    }

    // Drop seq=1
    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(1);
        println!("*** [SIMULATED NETWORK DISRUPTION: Packet seq=1 dropped] ***\n");
    }

    let mut bobs_expected_seq = 0;

    println!("--- Bob processes initial batch ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    println!("\n--- Alice processes ACKs & triggers Limited Transmit ---");
    drain_channel(&mut alice, &mut link, bob.address);

    println!("\n--- Bob processes Limited Transmit packet ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    println!("\n--- Alice processes DupACK #2 ---");
    drain_channel(&mut alice, &mut link, bob.address);

    println!("\n--- FINAL STATE ---");
    alice.print();
    bob.print();
}

fn test_limited_transmit_comparison() {
    println!("\n==================================================");
    println!("  COMPARISON: Limited Transmit OFF vs. ON");
    println!("==================================================\n");

    // ----------------------------------------------------
    // SCENARIO A: Limited Transmit OFF (Default TCP behavior)
    // ----------------------------------------------------
    println!("============================================");
    println!("  SCENARIO A: Limited Transmit = OFF");
    println!("============================================\n");

    let mut link = Link::new();
    link.capacity = 10;

    let mut alice = TCP::new("Alice", &mut link).with_limited_transmit(false);
    let mut bob = TCP::new("Bob", &mut link);

    println!("--- Alice sends 3 packets (seq 0, 1, 2) ---");
    for seq in 0..3 {
        alice.send(Packet::rand_data(seq, 20), bob.address, &mut link);
    }

    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(1); // Drop packet seq 1
        println!("*** [NETWORK LOSS: Packet seq=1 dropped] ***\n");
    }

    let mut bobs_expected_seq = 0;

    println!("--- Bob processes incoming batch ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    println!("\n--- Alice processes ACKs ---");
    drain_channel(&mut alice, &mut link, bob.address);

    println!("\n--- RESULT (LIMITED TRANSMIT OFF) ---");
    alice.print();
    println!("Notice: Alice only received 1 DupACK! Fast Retransmit CANNOT trigger.");
    println!("Alice is STUCK waiting for a Retransmission Timeout (RTO)!\n");

    // ----------------------------------------------------
    // SCENARIO B: Limited Transmit ON (RFC 3042 behavior)
    // ----------------------------------------------------
    println!("\n============================================");
    println!("  SCENARIO B: Limited Transmit = ON");
    println!("============================================\n");

    let mut link = Link::new();
    link.capacity = 10;

    let mut alice = TCP::new("Alice", &mut link).with_limited_transmit(true);
    let mut bob = TCP::new("Bob", &mut link);

    println!("--- Alice sends initial 3 packets (seq 0, 1, 2) ---");
    for seq in 0..3 {
        alice.send(Packet::rand_data(seq, 20), bob.address, &mut link);
    }

    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(1); // Drop packet seq 1
        println!("*** [NETWORK LOSS: Packet seq=1 dropped] ***\n");
    }

    let mut bobs_expected_seq = 0;

    println!("--- Bob processes initial batch (receives 0 & 2) ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    println!("\n--- Alice processes ACKs & triggers Limited Transmit ---");
    drain_channel(&mut alice, &mut link, bob.address);

    println!("\n--- Bob processes Limited Transmit packet (seq 3) ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    println!("\n--- Alice processes DupACK #2 (from seq 3) ---");
    drain_channel(&mut alice, &mut link, bob.address);

    println!("\n--- Bob receives seq 4 and generates DupACK #3 ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    println!("\n--- Alice receives DupACK #3 and triggers Fast Retransmit ---");
    drain_channel(&mut alice, &mut link, bob.address);

    println!("\n--- Bob receives retransmitted packet seq 1 ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    println!("\n--- Alice receives Full ACK and completes recovery ---");
    drain_channel(&mut alice, &mut link, bob.address);

    println!("\n--- RESULT (LIMITED TRANSMIT ON) ---");
    alice.print();
    println!("Success! Limited Transmit sent unsent data to generate enough DupACKs,");
    println!("allowing Fast Retransmit to recover the lost packet without an RTO delay!");
}

fn main() {
    test_limited_transmit();
    test_limited_transmit_comparison();
}
