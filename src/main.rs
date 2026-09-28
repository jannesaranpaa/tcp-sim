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
                    self.unacknowledged
                        .remove(self.unacknowledged.iter().position(|val| *val == ack_seq)?);
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

fn main() {
    let mut link = Link::new(); // capacity: 3
    let mut sender = TCP::new("Alice", &mut link);
    let mut receiver = TCP::new("Bob", &mut link);

    println!("== SIMULATING PACKET LOSS & TIMEOUT STATE ==\n");

    // 1. Alice sends 5 packets (0..=4) in one burst.
    // Packets 0, 1, 2 fit in Link. Packets 3 & 4 are DROPPED.
    println!("--- Alice sends 5 packets ---");
    for seq in 0..5 {
        sender.send(Packet::rand_data(seq, 20), receiver.address, &mut link);
    }

    sender.print();

    // 2. Bob reads all packets currently sitting in Link (0, 1, 2)
    // and sends an ACK back for each one.
    println!("--- Bob processes incoming queue ---");
    while let Some(packet) = receiver.receive(&mut link) {
        if let Packet::Data { seq, .. } = packet {
            receiver.send(Packet::new_ack(seq), sender.address, &mut link);
        }
    }

    // 3. Alice reads the ACKs sent by Bob from Link.
    println!("--- Alice processes incoming ACKs ---");
    while let Some(packet) = sender.receive(&mut link) {
        // TCP::receive automatically removes acked seq numbers from unacknowledged
    }

    // 4. Final state check
    println!("--- FINAL STATE ---");
    sender.print();
    receiver.print();
}
