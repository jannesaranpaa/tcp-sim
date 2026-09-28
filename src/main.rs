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
        }
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
        println!("│ Address:        {:<27}  │", self.address);
        println!("│ cwnd:           {:<27}  │", self.cwnd);
        println!("│ ssthresh:       {:<27}  │", self.ssthresh);
        println!("│ dup_acks:       {:<27}  │", self.dup_acks);
        println!("│ recover:        {:<27}  │", self.recover);
        println!("│ FlightSize:     {:<27}  │", self.flight_size());
        println!("│ unacknowledged: {:<27}  │", unack_str);
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

fn test_fast_retransmit() {
    println!("\n==========================================");
    println!("  TEST: 3x DupACK / Fast Retransmit");
    println!("==========================================\n");

    let mut link = Link::new();
    link.capacity = 10;

    let mut alice = TCP::new("Alice", &mut link);
    let mut bob = TCP::new("Bob", &mut link);

    // 1. Alice sends packets 0, 1, 2, 3, 4
    for seq in 0..5 {
        alice.send(Packet::rand_data(seq, 20), bob.address, &mut link);
    }

    // 2. Simulate packet loss in transit: drop packet 1
    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(1); // Drops seq=1
        println!("*** [SIMULATED NETWORK DISRUPTION: Packet seq=1 dropped] ***\n");
    }

    // 3. Bob processes incoming packets
    let mut bobs_expected_seq = 0;
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    // 4. Alice processes incoming ACKs from Bob
    println!("\n--- Alice processes ACKs ---");
    drain_channel(&mut alice, &mut link, bob.address);

    // 5. Bob receives the retransmitted packet seq=1 and ACKs up to 4
    println!("\n--- Bob receives retransmitted packet ---");
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    // 6. Alice processes the Full ACK and exits Fast Recovery
    println!("\n--- Alice processes Full ACK ---");
    drain_channel(&mut alice, &mut link, bob.address);

    // 7. Final State Check
    println!("\n--- FINAL POST-RECOVERY STATE ---");
    alice.print();
    bob.print();
}

fn main() {
    test_fast_retransmit();
}
