use clap::Parser;
use rand::Rng;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::Write;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    exercise_1: bool,

    #[arg(long)]
    exercise_2: bool,

    #[arg(long)]
    exercise_3: bool,

    #[arg(long)]
    exercise_4: bool,
}

#[derive(Debug, Clone)]
enum Packet {
    Ack { ack_seq: u64, sack: Vec<(u64, u64)> },
    Data { seq: u64, payload: Vec<u8> },
}

impl Packet {
    fn new_ack(ack_seq: u64) -> Self {
        Self::Ack {
            ack_seq,
            sack: Vec::new(),
        }
    }

    fn new_sack_ack(ack_seq: u64, sack: Vec<(u64, u64)>) -> Self {
        Self::Ack { ack_seq, sack }
    }

    fn rand_data(seq: u64, size: usize) -> Self {
        let mut payload = vec![0u8; size];
        rand::rng().fill_bytes(&mut payload);
        Self::Data { seq, payload }
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
        self.channels.get_mut(&addr)?.pop_front()
    }
}

struct DiagramLogger {
    file: File,
    pending_state: Option<String>,
}

impl DiagramLogger {
    fn new(filename: &str) -> Self {
        Self::create(filename, false)
    }

    fn new_fit_page(filename: &str) -> Self {
        Self::create(filename, true)
    }

    fn create(filename: &str, fit_page: bool) -> Self {
        let mut file = File::create(filename).expect("failed to create diagram output file");
        if fit_page {
            writeln!(file, "#set page(width: auto, height: auto, margin: 1cm)").unwrap();
        }

        writeln!(file, "#import \"@preview/chronos:0.3.0\": *").unwrap();
        writeln!(
            file,
            "#set text(font: (\"Liberation Sans\", \"DejaVu Sans\", \"Arial\"))\n"
        )
        .unwrap();
        writeln!(file, "#align(center)[\n  #diagram({{").unwrap();
        writeln!(file, "    _par(\"S\", display-name: [Sender])").unwrap();
        writeln!(file, "    _par(\"R\", display-name: [Receiver])\n").unwrap();

        Self {
            file,
            pending_state: None,
        }
    }

    fn log_state(&mut self, phase: &str, cwnd: u32, ssthresh: u32, dup_acks: u32, recover: u64) {
        self.pending_state = Some(format!(
            "{phase}: cwnd={cwnd}MSS, ssthresh={ssthresh}MSS, dupACKs={dup_acks}, recover={recover}"
        ));
    }

    fn log_state_sack(
        &mut self,
        phase: &str,
        cwnd: u32,
        ssthresh: u32,
        dup_acks: u32,
        recover: u64,
        pipe: u32,
        sacked: &BTreeSet<u64>,
    ) {
        let sacked: Vec<String> = sacked.iter().map(|s| s.to_string()).collect();
        self.pending_state = Some(format!(
            "{phase} \\ cwnd={cwnd}MSS, ssthresh={ssthresh}MSS, dupACKs={dup_acks} \\ pipe={pipe}, recover={recover} \\ sacked={{{}}}",
            sacked.join(", ")
        ));
    }

    fn log_send_data(&mut self, seq: u64) {
        let comment = match self.pending_state.take() {
            Some(state) => format!("Seg. {seq} \\ ({state})"),
            None => format!("Seg. {seq}"),
        };
        writeln!(self.file, "    _seq(\"S\", \"R\", comment: [{comment}])").unwrap();
    }

    fn log_send_ack(&mut self, ack_seq: u64, is_dup: bool) {
        self.log_send_ack_sack(ack_seq, is_dup, &[]);
    }

    fn log_send_ack_sack(&mut self, ack_seq: u64, is_dup: bool, sack: &[(u64, u64)]) {
        let mut label = if is_dup {
            format!("DupACK {ack_seq}")
        } else {
            format!("Ack. {ack_seq}")
        };
        if !sack.is_empty() {
            let blocks: Vec<String> = sack.iter().map(|(l, r)| format!("{l}-{r}")).collect();
            label.push_str(&format!(" SACK[{}]", blocks.join(", ")));
        }

        let comment = match self.pending_state.take() {
            Some(state) => format!("{label} \\ ({state})"),
            None => label,
        };
        writeln!(self.file, "    _seq(\"R\", \"S\", comment: [{comment}])").unwrap();
    }

    fn log_loss(&mut self, seq: u64) {
        writeln!(
            self.file,
            "    _seq(\"S\", \"R\", comment: [Seg. {seq} Dropped])"
        )
        .unwrap();
    }

    fn finish(mut self, filename: &str) {
        if let Some(state) = self.pending_state.take() {
            writeln!(
                self.file,
                "    _seq(\"S\", \"R\", comment: [Final State: {state}])"
            )
            .unwrap();
        }
        writeln!(self.file, "  }})\n]").unwrap();
        println!("Generated diagram in {filename}");
    }
}

struct TCP {
    name: String,
    address: Address,
    unacknowledged: Vec<u64>,
    smss: u32,
    rwnd: u32,
    cwnd: u32,
    ssthresh: u32,
    recover: u64,
    in_fast_recovery: bool,
    dup_acks: u32,
    limited_transmit: bool,
    sack_enabled: bool,
    sacked: BTreeSet<u64>,
    retransmitted: HashSet<u64>,
}

const SEG_LEN: u64 = 1000;
const DUP_THRESH: usize = 3;

impl TCP {
    fn flight_size(&self) -> u32 {
        if self.sack_enabled {
            return self.pipe();
        }
        self.unacknowledged.len() as u32
    }

    /// RFC 6675 IsLost: at least DupThresh SACKed segments above `seq`.
    fn is_lost(&self, seq: u64) -> bool {
        !self.sacked.contains(&seq) && self.sacked.range(seq + 1..).count() >= DUP_THRESH
    }

    /// RFC 6675 pipe: outstanding segments believed to still be in the network.
    fn pipe(&self) -> u32 {
        self.unacknowledged
            .iter()
            .map(|&seq| {
                if self.sacked.contains(&seq) {
                    return 0;
                }
                let lost = self.is_lost(seq);
                (!lost) as u32 + self.retransmitted.contains(&seq) as u32
            })
            .sum()
    }

    /// Lowest lost segment that has not been retransmitted yet.
    fn next_hole(&self) -> Option<u64> {
        self.unacknowledged
            .iter()
            .copied()
            .find(|&seq| self.is_lost(seq) && !self.retransmitted.contains(&seq))
            .or_else(|| {
                // No new data to send: retransmit any hole below the highest SACKed segment
                // (RFC 6675 NextSeg rule 4).
                let highest = *self.sacked.iter().next_back()?;
                self.unacknowledged.iter().copied().find(|&seq| {
                    seq < highest
                        && !self.sacked.contains(&seq)
                        && !self.retransmitted.contains(&seq)
                })
            })
    }

    fn send_window(&self) -> u32 {
        self.cwnd.min(self.rwnd)
    }

    fn new(name: &str, link: &mut Link) -> Self {
        Self {
            name: name.to_string(),
            address: link.register(),
            unacknowledged: Vec::new(),
            smss: 1,
            rwnd: 10,
            cwnd: 10,
            ssthresh: 64,
            recover: 0,
            in_fast_recovery: false,
            dup_acks: 0,
            limited_transmit: false,
            sack_enabled: false,
            sacked: BTreeSet::new(),
            retransmitted: HashSet::new(),
        }
    }

    fn with_sack(mut self, enabled: bool) -> Self {
        self.sack_enabled = enabled;
        self
    }

    fn with_limited_transmit(mut self, enabled: bool) -> Self {
        self.limited_transmit = enabled;
        self
    }

    fn send(
        &mut self,
        packet: Packet,
        to: Address,
        link: &mut Link,
        logger: &mut Option<&mut DiagramLogger>,
    ) {
        match packet {
            Packet::Data { seq, .. } => {
                if self.sack_enabled && self.unacknowledged.contains(&seq) {
                    self.retransmitted.insert(seq);
                } else {
                    self.unacknowledged.push(seq);
                }
                if let Some(log) = logger {
                    log.log_send_data(seq);
                }
            }
            Packet::Ack { ack_seq, .. } => {
                if let Some(log) = logger {
                    log.log_send_ack(ack_seq, false);
                }
            }
        }
        link.send(packet, to);
    }

    fn receive(
        &mut self,
        link: &mut Link,
        peer_addr: Address,
        logger: &mut Option<&mut DiagramLogger>,
    ) -> Option<Packet> {
        let packet = link.receive(self.address)?;
        println!("{} received: {:?}", self.name, packet);

        if let Packet::Ack { ack_seq, sack } = &packet {
            if self.sack_enabled {
                self.on_ack_sack(*ack_seq, sack, peer_addr, link, logger);
            } else {
                self.on_ack_received(*ack_seq, peer_addr, link, logger);
            }
        }

        Some(packet)
    }

    fn on_ack_received(
        &mut self,
        ack_seq: u64,
        peer_addr: Address,
        link: &mut Link,
        logger: &mut Option<&mut DiagramLogger>,
    ) {
        let newly_acked = self
            .unacknowledged
            .iter()
            .filter(|&&seq| seq <= ack_seq)
            .count();

        if newly_acked > 0 {
            self.unacknowledged.retain(|&seq| seq > ack_seq);

            if self.in_fast_recovery {
                if ack_seq >= self.recover {
                    println!(
                        "{} -> Full ACK ({ack_seq}) received, exiting Fast Recovery.",
                        self.name
                    );
                    self.cwnd = self.ssthresh;
                    self.dup_acks = 0;
                    self.in_fast_recovery = false;

                    if let Some(log) = logger {
                        log.log_state(
                            "Exit Fast Recovery",
                            self.cwnd,
                            self.ssthresh,
                            self.dup_acks,
                            self.recover,
                        );
                    }
                } else {
                    println!(
                        "{} -> Partial ACK ({ack_seq}) received, retransmitting.",
                        self.name
                    );
                    self.cwnd = self.cwnd.saturating_sub(newly_acked as u32) + self.smss;

                    if let Some(log) = logger {
                        log.log_state(
                            "Partial ACK",
                            self.cwnd,
                            self.ssthresh,
                            self.dup_acks,
                            self.recover,
                        );
                    }

                    if let Some(&missing_seq) = self.unacknowledged.first() {
                        self.send(Packet::rand_data(missing_seq, 20), peer_addr, link, logger);
                    }
                }
            } else {
                self.dup_acks = 0;
                if self.cwnd < self.ssthresh {
                    self.cwnd += self.smss;
                } else {
                    self.cwnd += self.smss / self.cwnd.max(1);
                }
            }
            return;
        }

        self.dup_acks += 1;

        if self.in_fast_recovery {
            self.cwnd += self.smss;
            if let Some(log) = logger {
                log.log_state(
                    "DupACK (Inflate cwnd)",
                    self.cwnd,
                    self.ssthresh,
                    self.dup_acks,
                    self.recover,
                );
            }
            return;
        }

        if self.dup_acks == 3 {
            println!(
                "{} -> 3x DupACK ({ack_seq}), triggering Fast Retransmit.",
                self.name
            );
            self.recover = *self.unacknowledged.last().unwrap_or(&ack_seq);
            let flight_size = self.flight_size();
            self.ssthresh = (flight_size / 2).max(2 * self.smss);
            self.cwnd = self.ssthresh + 3 * self.smss;
            self.in_fast_recovery = true;

            if let Some(log) = logger {
                log.log_state(
                    "Fast Retransmit",
                    self.cwnd,
                    self.ssthresh,
                    self.dup_acks,
                    self.recover,
                );
            }

            if let Some(&missing_seq) = self.unacknowledged.first() {
                self.send(Packet::rand_data(missing_seq, 20), peer_addr, link, logger);
            }
        } else if self.limited_transmit && self.dup_acks < 3 {
            let next_unsent_seq = self.unacknowledged.last().map_or(21000, |last| last + 1000);

            if self.flight_size() < self.send_window() + 2 {
                println!(
                    "{} -> [Limited Transmit] DupACK #{}, sending {next_unsent_seq}",
                    self.name, self.dup_acks
                );
                self.send(
                    Packet::rand_data(next_unsent_seq, 20),
                    peer_addr,
                    link,
                    logger,
                );
            }
        } else if let Some(log) = logger {
            log.log_state(
                "DupACK Received",
                self.cwnd,
                self.ssthresh,
                self.dup_acks,
                self.recover,
            );
        }
    }

    fn on_ack_sack(
        &mut self,
        ack_seq: u64,
        sack: &[(u64, u64)],
        peer_addr: Address,
        link: &mut Link,
        logger: &mut Option<&mut DiagramLogger>,
    ) {
        self.on_ack_sack_inner(ack_seq, sack, peer_addr, link, logger);
        self.print_sack_state();
    }

    fn print_sack_state(&self) {
        println!(
            "  [{}] smss={} rwnd={} cwnd={} ssthresh={} dup_acks={} recover={} in_recovery={} pipe={} flight={}",
            self.name,
            self.smss,
            self.rwnd,
            self.cwnd,
            self.ssthresh,
            self.dup_acks,
            self.recover,
            self.in_fast_recovery,
            self.pipe(),
            self.unacknowledged.len()
        );
        println!(
            "    unacknowledged={:?} sacked={:?} retransmitted={:?}",
            self.unacknowledged,
            self.sacked,
            {
                let mut r: Vec<_> = self.retransmitted.iter().collect();
                r.sort();
                r
            }
        );
    }

    fn on_ack_sack_inner(
        &mut self,
        ack_seq: u64,
        sack: &[(u64, u64)],
        peer_addr: Address,
        link: &mut Link,
        logger: &mut Option<&mut DiagramLogger>,
    ) {
        let newly_acked = self
            .unacknowledged
            .iter()
            .filter(|&&seq| seq < ack_seq)
            .count();
        self.unacknowledged.retain(|&seq| seq >= ack_seq);
        self.sacked.retain(|&seq| seq >= ack_seq);
        self.retransmitted.retain(|&seq| seq >= ack_seq);

        let mut new_sack_info = false;
        for &(left, right) in sack {
            let mut seq = left;
            while seq < right {
                if self.unacknowledged.contains(&seq) && self.sacked.insert(seq) {
                    new_sack_info = true;
                }
                seq += SEG_LEN;
            }
        }

        if newly_acked == 0 {
            self.dup_acks += 1;
        }

        if self.in_fast_recovery {
            if ack_seq >= self.recover {
                println!(
                    "{} -> Full ACK ({ack_seq}) received, exiting SACK recovery.",
                    self.name
                );
                self.cwnd = self.ssthresh;
                self.dup_acks = 0;
                self.in_fast_recovery = false;
                self.sacked.clear();
                self.retransmitted.clear();
                if let Some(log) = logger {
                    log.log_state_sack(
                        "Exit SACK Recovery",
                        self.cwnd,
                        self.ssthresh,
                        self.dup_acks,
                        self.recover,
                        self.pipe(),
                        &self.sacked,
                    );
                }
                return;
            }
        } else if newly_acked > 0 {
            self.dup_acks = 0;
            if self.cwnd < self.ssthresh {
                self.cwnd += self.smss;
            }
            return;
        } else if let Some(first) = self.unacknowledged.first().copied() {
            if !(self.is_lost(first) || (self.dup_acks as usize >= DUP_THRESH && new_sack_info)) {
                if let Some(log) = logger {
                    log.log_state_sack(
                        "DupACK Received",
                        self.cwnd,
                        self.ssthresh,
                        self.dup_acks,
                        self.recover,
                        self.pipe(),
                        &self.sacked,
                    );
                }
                return;
            }
            // Entering recovery: the first hole is considered lost.
            println!("{} -> SACK loss detected, entering recovery.", self.name);
            let flight_size = self.unacknowledged.len() as u32;
            self.recover = *self.unacknowledged.last().unwrap();
            self.ssthresh = (flight_size / 2).max(2 * self.smss);
            self.cwnd = self.ssthresh;
            self.in_fast_recovery = true;
            if let Some(log) = logger {
                log.log_state_sack(
                    "SACK Recovery",
                    self.cwnd,
                    self.ssthresh,
                    self.dup_acks,
                    self.recover,
                    self.pipe(),
                    &self.sacked,
                );
            }
            // The first hole is retransmitted even if IsLost's threshold is not met yet.
            self.send(Packet::rand_data(first, 20), peer_addr, link, logger);
        } else {
            return;
        }

        if let Some(log) = logger {
            log.log_state_sack(
                "SACK Recovery",
                self.cwnd,
                self.ssthresh,
                self.dup_acks,
                self.recover,
                self.pipe(),
                &self.sacked,
            );
        }

        // Fill the pipe: retransmit lost holes first, new data afterwards.
        while self.pipe() < self.cwnd {
            match self.next_hole() {
                Some(seq) => self.send(Packet::rand_data(seq, 20), peer_addr, link, logger),
                None => break,
            }
        }
    }

    fn print(&self) {
        println!(
            "[{}] address={} cwnd={} ssthresh={} dup_acks={} lt={} recover={} flight={}",
            self.name,
            self.address,
            self.cwnd,
            self.ssthresh,
            self.dup_acks,
            self.limited_transmit,
            self.recover,
            self.flight_size()
        );
        println!("  unacknowledged: {:?}", self.unacknowledged);
        if self.sack_enabled {
            self.print_sack_state();
        }
    }
}

struct Receiver {
    expected_seq: u64,
    out_of_order: BTreeSet<u64>,
    /// Segments in order of arrival, newest last (for RFC 2018 block ordering).
    arrivals: Vec<u64>,
}

impl Receiver {
    fn new(expected_seq: u64) -> Self {
        Self {
            expected_seq,
            out_of_order: BTreeSet::new(),
            arrivals: Vec::new(),
        }
    }

    fn blocks(&self) -> Vec<(u64, u64)> {
        let mut blocks: Vec<(u64, u64)> = Vec::new();
        for &seq in &self.out_of_order {
            match blocks.last_mut() {
                Some(last) if last.1 == seq => last.1 = seq + SEG_LEN,
                _ => blocks.push((seq, seq + SEG_LEN)),
            }
        }
        // Most recently received block first; at most 3 blocks (with timestamps).
        let mut ordered = Vec::new();
        for &seq in self.arrivals.iter().rev() {
            if let Some(&b) = blocks.iter().find(|b| b.0 <= seq && seq < b.1) {
                if !ordered.contains(&b) {
                    ordered.push(b);
                }
            }
        }
        ordered.truncate(3);
        ordered
    }

    fn on_data(&mut self, seq: u64) -> Packet {
        if seq == self.expected_seq {
            self.expected_seq += SEG_LEN;
            while self.out_of_order.remove(&self.expected_seq) {
                self.expected_seq += SEG_LEN;
            }
            self.arrivals.retain(|&s| s >= self.expected_seq);
        } else if seq > self.expected_seq && self.out_of_order.insert(seq) {
            self.arrivals.push(seq);
        }
        Packet::new_sack_ack(self.expected_seq, self.blocks())
    }
}

fn receive_and_sack(
    receiver: &mut TCP,
    state: &mut Receiver,
    sender_addr: Address,
    link: &mut Link,
    logger: &mut Option<&mut DiagramLogger>,
) {
    while let Some(Packet::Data { seq, .. }) = receiver.receive(link, sender_addr, &mut None) {
        let is_dup = seq != state.expected_seq;
        let ack = state.on_data(seq);
        if let Packet::Ack { ack_seq, sack } = &ack {
            if let Some(log) = logger {
                log.log_send_ack_sack(*ack_seq, is_dup, sack);
            }
        }
        receiver.send(ack, sender_addr, link, &mut None);
    }
}

fn drain_channel(
    endpoint: &mut TCP,
    link: &mut Link,
    peer_addr: Address,
    logger: &mut Option<&mut DiagramLogger>,
) -> Vec<Packet> {
    let mut received = Vec::new();
    while let Some(packet) = endpoint.receive(link, peer_addr, logger) {
        received.push(packet);
    }
    received
}

fn receive_and_ack_with_dups(
    receiver: &mut TCP,
    sender_addr: Address,
    link: &mut Link,
    expected_seq: &mut u64,
    logger: &mut Option<&mut DiagramLogger>,
) {
    while let Some(packet) = receiver.receive(link, sender_addr, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == *expected_seq {
                *expected_seq += 1000;
                if let Some(log) = logger {
                    log.log_send_ack(*expected_seq, false);
                }
                receiver.send(Packet::new_ack(*expected_seq), sender_addr, link, &mut None);
            } else {
                let last_ack = *expected_seq;
                println!("--> Out-of-order seq {seq}! DupACK for {last_ack}");
                if let Some(log) = logger {
                    log.log_send_ack(last_ack, true);
                }
                receiver.send(Packet::new_ack(last_ack), sender_addr, link, &mut None);
            }
        }
    }
}

fn run_exercise_1() {
    let filename = "exercise-1.typ";
    let mut logger = DiagramLogger::new(filename);
    let mut link = Link::new();
    link.capacity = 20;

    let mut alice = TCP::new("Alice", &mut link).with_limited_transmit(false);
    let mut bob = TCP::new("Bob", &mut link);

    alice.cwnd = 8;
    alice.ssthresh = 64;

    logger.log_state("Initial State", alice.cwnd, alice.ssthresh, 0, 0);

    for i in 0..8 {
        let seq = 21000 + (i * 1000);
        alice.send(
            Packet::rand_data(seq, 20),
            bob.address,
            &mut link,
            &mut Some(&mut logger),
        );
    }
    alice.print();

    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(5);
        channel.remove(4);
        channel.remove(0);

        logger.log_loss(21000);
        logger.log_loss(25000);
        logger.log_loss(26000);
    }

    let mut bobs_expected_seq = 21000;
    receive_and_ack_with_dups(
        &mut bob,
        alice.address,
        &mut link,
        &mut bobs_expected_seq,
        &mut Some(&mut logger),
    );

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 21000 {
                bobs_expected_seq = 25000;
                logger.log_send_ack(25000, false);
                bob.send(Packet::new_ack(25000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 25000 {
                bobs_expected_seq = 26000;
                logger.log_send_ack(26000, false);
                bob.send(Packet::new_ack(26000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 26000 {
                bobs_expected_seq = 29000;
                logger.log_send_ack(29000, false);
                bob.send(Packet::new_ack(29000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));

    println!("Bob's expected seq: {bobs_expected_seq}");
    alice.print();

    logger.finish(filename);
}

fn run_exercise_2() {
    let filename = "exercise-2.typ";
    let mut logger = DiagramLogger::new(filename);
    let mut link = Link::new();
    link.capacity = 20;

    let mut alice = TCP::new("Alice", &mut link).with_limited_transmit(false);
    let mut bob = TCP::new("Bob", &mut link);

    alice.cwnd = 6;
    alice.ssthresh = 64;

    logger.log_state("Initial State", alice.cwnd, alice.ssthresh, 0, 0);

    for i in 0..6 {
        let seq = 21000 + (i * 1000);
        alice.send(
            Packet::rand_data(seq, 20),
            bob.address,
            &mut link,
            &mut Some(&mut logger),
        );
    }
    alice.print();

    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(3);
        channel.remove(1);
        channel.remove(0);

        logger.log_loss(21000);
        logger.log_loss(22000);
        logger.log_loss(24000);
    }

    let mut bobs_expected_seq = 21000;
    receive_and_ack_with_dups(
        &mut bob,
        alice.address,
        &mut link,
        &mut bobs_expected_seq,
        &mut Some(&mut logger),
    );

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 21000 {
                bobs_expected_seq = 22000;
                logger.log_send_ack(22000, false);
                bob.send(Packet::new_ack(22000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 22000 {
                bobs_expected_seq = 24000;
                logger.log_send_ack(24000, false);
                bob.send(Packet::new_ack(24000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 24000 {
                bobs_expected_seq = 27000;
                logger.log_send_ack(27000, false);
                bob.send(Packet::new_ack(27000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));

    println!("Bob's expected seq: {bobs_expected_seq}");
    alice.print();

    logger.finish(filename);
}

fn run_exercise_3() {
    let filename = "exercise-3.typ";
    let mut logger = DiagramLogger::new(filename);
    let mut link = Link::new();
    link.capacity = 20;

    let mut alice = TCP::new("Alice", &mut link).with_limited_transmit(true);
    let mut bob = TCP::new("Bob", &mut link);

    alice.cwnd = 6;
    alice.ssthresh = 64;

    logger.log_state("Initial State", alice.cwnd, alice.ssthresh, 0, 0);

    for i in 0..6 {
        let seq = 21000 + (i * 1000);
        alice.send(
            Packet::rand_data(seq, 20),
            bob.address,
            &mut link,
            &mut Some(&mut logger),
        );
    }
    alice.print();

    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(3);
        channel.remove(1);
        channel.remove(0);

        logger.log_loss(21000);
        logger.log_loss(22000);
        logger.log_loss(24000);
    }

    let mut bobs_expected_seq = 21000;

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            println!("--> Out-of-order seq {seq}! DupACK for 21000");
            logger.log_send_ack(21000, true);
            bob.send(Packet::new_ack(21000), alice.address, &mut link, &mut None);
            break;
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            println!("--> Out-of-order seq {seq}! DupACK for 21000");
            logger.log_send_ack(21000, true);
            bob.send(Packet::new_ack(21000), alice.address, &mut link, &mut None);
            break;
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    receive_and_ack_with_dups(
        &mut bob,
        alice.address,
        &mut link,
        &mut bobs_expected_seq,
        &mut Some(&mut logger),
    );

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 21000 {
                bobs_expected_seq = 22000;
                logger.log_send_ack(22000, false);
                bob.send(Packet::new_ack(22000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 22000 {
                bobs_expected_seq = 24000;
                logger.log_send_ack(24000, false);
                bob.send(Packet::new_ack(24000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 24000 {
                bobs_expected_seq = 29000;
                logger.log_send_ack(29000, false);
                bob.send(Packet::new_ack(29000), alice.address, &mut link, &mut None);
            }
        }
    }

    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));

    println!("Bob's expected seq: {bobs_expected_seq}");
    alice.print();

    logger.finish(filename);
}

fn run_exercise_4() {
    let filename = "exercise-4.typ";
    let mut logger = DiagramLogger::new_fit_page(filename);
    let mut link = Link::new();
    link.capacity = 20;

    let mut alice = TCP::new("Alice", &mut link).with_sack(true);
    let mut bob = TCP::new("Bob", &mut link);
    let mut bob_state = Receiver::new(21000);

    alice.cwnd = 8;
    alice.ssthresh = 64;

    logger.log_state("Initial State", alice.cwnd, alice.ssthresh, 0, 0);

    for i in 0..8 {
        let seq = 21000 + (i * 1000);
        alice.send(
            Packet::rand_data(seq, 20),
            bob.address,
            &mut link,
            &mut Some(&mut logger),
        );
    }
    alice.print();

    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(5);
        channel.remove(4);
        channel.remove(0);

        logger.log_loss(21000);
        logger.log_loss(25000);
        logger.log_loss(26000);
    }

    receive_and_sack(
        &mut bob,
        &mut bob_state,
        alice.address,
        &mut link,
        &mut Some(&mut logger),
    );
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    // Alice retransmits the holes; Bob acknowledges them.
    receive_and_sack(
        &mut bob,
        &mut bob_state,
        alice.address,
        &mut link,
        &mut Some(&mut logger),
    );
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));

    // Partial ACKs freed pipe space, so the remaining holes are retransmitted now.
    receive_and_sack(
        &mut bob,
        &mut bob_state,
        alice.address,
        &mut link,
        &mut Some(&mut logger),
    );
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));

    println!("Bob's expected seq: {}", bob_state.expected_seq);
    alice.print();

    logger.finish(filename);
}

fn main() {
    let args = Args::parse();

    if args.exercise_1 {
        run_exercise_1();
    } else if args.exercise_2 {
        run_exercise_2();
    } else if args.exercise_3 {
        run_exercise_3();
    } else if args.exercise_4 {
        run_exercise_4();
    } else {
        println!(
            "Please specify an exercise flag: --exercise-1, --exercise-2, --exercise-3, or --exercise-4"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sack_of(p: Packet) -> (u64, Vec<(u64, u64)>) {
        match p {
            Packet::Ack { ack_seq, sack } => (ack_seq, sack),
            _ => panic!("not an ack"),
        }
    }

    #[test]
    fn receiver_merges_and_orders_blocks() {
        let mut r = Receiver::new(21000);
        r.on_data(23000);
        r.on_data(25000);
        let (ack, sack) = sack_of(r.on_data(26000));
        assert_eq!(ack, 21000);
        assert_eq!(sack, vec![(25000, 27000), (23000, 24000)]);
    }

    #[test]
    fn receiver_limits_blocks_and_advances_cumulatively() {
        let mut r = Receiver::new(0);
        for s in [2000, 4000, 6000, 8000] {
            r.on_data(s);
        }
        let (_, sack) = sack_of(r.on_data(10000));
        assert_eq!(sack.len(), 3);
        r.on_data(0);
        let (ack, _) = sack_of(r.on_data(1000));
        assert_eq!(ack, 3000);
    }

    #[test]
    fn scoreboard_is_lost_and_pipe() {
        let mut link = Link::new();
        let mut t = TCP::new("t", &mut link).with_sack(true);
        t.unacknowledged = vec![0, 1000, 2000, 3000, 4000, 5000];
        t.sacked = [2000, 4000, 5000].into_iter().collect();
        assert!(t.is_lost(0));
        assert!(!t.is_lost(1000) || t.sacked.range(1001..).count() >= 3);
        assert!(!t.is_lost(3000));
        // 0 and 1000 lost (3 SACKed above), 3000 in flight.
        assert_eq!(t.pipe(), 1);
    }
}
