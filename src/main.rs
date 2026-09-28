use rand::Rng;
use std::collections::{HashMap, VecDeque};

#[derive(Debug, Clone)]
enum Packet {
    Ack { ack_seq: u64 },
    Data { seq: u64, payload: Vec<u8> },
}

// struct Packet(Vec<u8>);

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
        // Simulate dropping if capacity is full

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
    smss: u32,
    rmss: u32,
    rwnd: u32,
    cwnd: u32,
    iw: u32,
    lw: u32,
    rw: u32,

    unacknowledged: Vec<u64>,
    address: Address,

    dup_ack_count: HashMap<u64, u32>,
}

impl TCP {
    fn new(name: &str, link: &mut Link) -> Self {
        Self {
            name: String::from(name),
            smss: 1000,
            rmss: 1000,
            rwnd: 10,
            cwnd: 10,
            iw: 1,
            lw: 1,
            rw: 1,

            unacknowledged: vec![],
            address: link.register(),

            dup_ack_count: HashMap::new(),
        }
    }

    fn send(&mut self, data: Packet, to: Address, link: &mut Link) {
        match data {
            Packet::Data { seq, .. } => self.unacknowledged.push(seq),
            _ => (),
        }

        link.send(data, to);
    }

    fn receive(&mut self, link: &mut Link) -> Option<Packet> {
        if let Some(data) = link.receive(self.address) {
            println!("{} received: {:?}", self.name, data);

            match data {
                Packet::Ack { ack_seq } => {
                    if let Some(idx) = self.unacknowledged.iter().position(|val| *val == ack_seq) {
                        self.unacknowledged.remove(idx);
                        self.dup_ack_count.remove(&ack_seq);
                    } else {
                        let count = self.dup_ack_count.entry(ack_seq).or_insert(0);
                        *count += 1;

                        if *count == 3 {
                            println!(
                                "{} -> FAST RETRANSMISSION triggered for {}",
                                self.name, ack_seq
                            );
                        }
                    }
                }
                _ => (),
            }

            Some(data)
        } else {
            None
        }
    }

    fn print(&self) {
        println!("{}", self.name);
        println!("cwnd: {}", self.cwnd);
        println!("unackd: {:?}", self.unacknowledged);
        println!("");
    }
}

fn drain_channel(endpoint: &mut TCP, link: &mut Link) -> Vec<Packet> {
    let mut received = Vec::new();
    while let Some(packet) = endpoint.receive(link) {
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
    while let Some(packet) = receiver.receive(link) {
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
    link.capacity = 10; // Large capacity so packets aren't dropped by Link queue size

    let mut alice = TCP::new("Alice", &mut link);
    let mut bob = TCP::new("Bob", &mut link);

    // 1. Alice sends packets 0, 1, 2, 3, 4
    for seq in 0..5 {
        alice.send(Packet::rand_data(seq, 20), bob.address, &mut link);
    }

    // 2. Simulate packet loss in transit: manually remove packet 1 from Bob's channel
    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(1); // Drops packet with seq=1
        println!("*** [SIMULATED NETWORK DISRUPTION: Packet seq=1 dropped] ***\n");
    }

    // 3. Bob processes incoming packets:
    // Receives 0 (In-order) -> ACKs 0 (Expected becomes 1)
    // Receives 2 (Out-of-order!) -> Sends DupACK 0
    // Receives 3 (Out-of-order!) -> Sends DupACK 0
    // Receives 4 (Out-of-order!) -> Sends DupACK 0 (Total: 3 DupACKs for seq 0)
    let mut bobs_expected_seq = 0;
    receive_and_ack_with_dups(&mut bob, alice.address, &mut link, &mut bobs_expected_seq);

    // 4. Alice processes all incoming ACKs from Bob
    println!("\n--- Alice processes ACKs ---");
    drain_channel(&mut alice, &mut link);

    // 5. Final State Check
    println!("\n--- FINAL STATE ---");
    alice.print();
    bob.print();
}

fn main() {
    test_fast_retransmit();
}
