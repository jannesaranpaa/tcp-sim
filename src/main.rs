use clap::Parser;
use rand::Rng;
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::Write;

/// TCP Simulation Suite
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Run the RFC 6582/3782 NewReno exercise 1 scenario and write output to exercise-1.typ
    #[arg(long)]
    exercise_1: bool,

    /// Run the RFC 6582/3782 NewReno exercise 2 scenario and write output to exercise-2.typ
    #[arg(long)]
    exercise_2: bool,

    /// Run Exercise 3: Exercise 2 scenario WITH Limited Transmit to exercise-3.typ
    #[arg(long)]
    exercise_3: bool,
}

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

/// DiagramLogger generating clean inline sequence comments for chronos:0.3.0
struct DiagramLogger {
    file: File,
    pending_state: Option<String>,
}

impl DiagramLogger {
    fn new(filename: &str) -> Self {
        let mut file = File::create(filename).expect("Failed to create diagram file");

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

    fn log_send_data(&mut self, seq: u64) {
        let comment = if let Some(state) = self.pending_state.take() {
            format!("Seg. {} \\ ({})", seq, state)
        } else {
            format!("Seg. {}", seq)
        };
        writeln!(self.file, "    _seq(\"S\", \"R\", comment: [{}])", comment).unwrap();
    }

    fn log_send_ack(&mut self, ack_seq: u64, is_dup: bool) {
        let ack_label = if is_dup {
            format!("DupACK {}", ack_seq)
        } else {
            format!("Ack. {}", ack_seq)
        };

        let comment = if let Some(state) = self.pending_state.take() {
            format!("{} \\ ({})", ack_label, state)
        } else {
            ack_label
        };
        writeln!(self.file, "    _seq(\"R\", \"S\", comment: [{}])", comment).unwrap();
    }

    fn log_loss(&mut self, seq: u64) {
        writeln!(
            self.file,
            "    _seq(\"S\", \"R\", comment: [Seg. {} Dropped])",
            seq
        )
        .unwrap();
    }

    fn finish(mut self, filename: &str) {
        if let Some(state) = self.pending_state.take() {
            writeln!(
                self.file,
                "    _seq(\"S\", \"R\", comment: [Final State: {}])",
                state
            )
            .unwrap();
        }
        writeln!(self.file, "  }})\n]").unwrap();
        println!("Generated valid Typst diagram in {}!", filename);
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

            smss: 1,

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

    fn send(
        &mut self,
        data: Packet,
        to: Address,
        link: &mut Link,
        logger: &mut Option<&mut DiagramLogger>,
    ) {
        if let Packet::Data { seq, .. } = data {
            self.unacknowledged.push(seq);
            if let Some(log) = logger {
                log.log_send_data(seq);
            }
        } else if let Packet::Ack { ack_seq } = data {
            if let Some(log) = logger {
                log.log_send_ack(ack_seq, false);
            }
        }

        link.send(data, to);
    }

    fn receive(
        &mut self,
        link: &mut Link,
        peer_addr: Address,
        logger: &mut Option<&mut DiagramLogger>,
    ) -> Option<Packet> {
        let data = link.receive(self.address)?;
        println!("{} received: {:?}", self.name, data);

        if let Packet::Ack { ack_seq } = data {
            self.on_ack_received(ack_seq, peer_addr, link, logger);
        }

        Some(data)
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
                    // FULL ACK: Exits Fast Recovery
                    println!(
                        "{} -> Full ACK ({}) received! Exiting Fast Recovery.",
                        self.name, ack_seq
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
                    // PARTIAL ACK: Retransmit next unacknowledged packet immediately
                    println!(
                        "{} -> Partial ACK ({}) received! Retransmitting next missing packet.",
                        self.name, ack_seq
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
                        println!(
                            "{} -> [Partial ACK Retransmit] Sending seq {}",
                            self.name, missing_seq
                        );
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
        } else {
            // --- DUPLICATE ACK ---
            self.dup_acks += 1;

            if !self.in_fast_recovery {
                if self.dup_acks == 3 {
                    // TRIGGER FAST RETRANSMISSION & ENTER FAST RECOVERY
                    println!(
                        "{} -> 3x DupACK ({})! Triggering Fast Retransmit.",
                        self.name, ack_seq
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
                        println!(
                            "{} -> [Fast Retransmit] Resending lost packet seq {}",
                            self.name, missing_seq
                        );
                        self.send(Packet::rand_data(missing_seq, 20), peer_addr, link, logger);
                    }
                } else if self.limited_transmit && self.dup_acks < 3 {
                    let next_unsent_seq =
                        self.unacknowledged.last().map_or(21000, |last| last + 1000);

                    if self.flight_size() < self.send_window() + 2 {
                        println!(
                            "{} -> [Limited Transmit] DupACK #{} received! Transmitting new seq {}",
                            self.name, self.dup_acks, next_unsent_seq
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
            } else {
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
                println!(
                    "--> Out-of-order seq {}! Sending duplicate ACK for {}",
                    seq, last_ack
                );
                if let Some(log) = logger {
                    log.log_send_ack(last_ack, true);
                }
                receiver.send(Packet::new_ack(last_ack), sender_addr, link, &mut None);
            }
        }
    }
}

fn run_exercise_3() {
    let filename = "exercise-3.typ";
    println!("\n==================================================");
    println!("  EXERCISE 3 SCENARIO: cwnd=6 MSS WITH Limited Transmit (RFC 3042)");
    println!("  Outputting diagram to: {}", filename);
    println!("==================================================\n");

    let mut logger = DiagramLogger::new(filename);
    let mut link = Link::new();
    link.capacity = 20;

    let mut alice = TCP::new("Alice", &mut link).with_limited_transmit(true);
    let mut bob = TCP::new("Bob", &mut link);

    alice.cwnd = 6;
    alice.ssthresh = 64;

    logger.log_state("Initial State", alice.cwnd, alice.ssthresh, 0, 0);

    // 1. Alice sends initial 6 MSS segments (21000..26000)
    println!("--- Step 1: Alice transmits initial window of 6 segments (21000..26000) ---");
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

    // 2. Network drops 1st (21000), 2nd (22000), and 4th (24000)
    if let Some(channel) = link.channels.get_mut(&bob.address) {
        channel.remove(3); // Removes 24000 (4th)
        channel.remove(1); // Removes 22000 (2nd)
        channel.remove(0); // Removes 21000 (1st)

        logger.log_loss(21000);
        logger.log_loss(22000);
        logger.log_loss(24000);

        println!(
            "*** [NETWORK LOSS: Segments 21000 (1st), 22000 (2nd), and 24000 (4th) dropped] ***\n"
        );
    }

    // 3. Bob receives Seg 3 (23000) -> DupACK 21000 (#1)
    let mut bobs_expected_seq = 21000;
    println!("--- Step 2: Bob receives Seg 3 (23000) -> Sends DupACK #1 ---");
    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            println!(
                "--> Out-of-order seq {}! Sending duplicate ACK for 21000",
                seq
            );
            logger.log_send_ack(21000, true);
            bob.send(Packet::new_ack(21000), alice.address, &mut link, &mut None);
            break;
        }
    }

    // 4. Alice receives DupACK #1 -> Limited Transmit sends Seg 7 (27000)
    println!("\n--- Step 3: Alice processes DupACK #1 -> Limited Transmit sends Seg 7 (27000) ---");
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    // 5. Bob receives Seg 5 (25000) -> Sends DupACK #2
    println!("\n--- Step 4: Bob receives Seg 5 (25000) -> Sends DupACK #2 ---");
    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            println!(
                "--> Out-of-order seq {}! Sending duplicate ACK for 21000",
                seq
            );
            logger.log_send_ack(21000, true);
            bob.send(Packet::new_ack(21000), alice.address, &mut link, &mut None);
            break;
        }
    }

    // 6. Alice receives DupACK #2 -> Limited Transmit sends Seg 8 (28000)
    println!("\n--- Step 5: Alice processes DupACK #2 -> Limited Transmit sends Seg 8 (28000) ---");
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    // 7. Bob receives surviving Seg 6 (26000), Seg 7 (27000), Seg 8 (28000) -> DupACKs #3, #4, #5
    println!(
        "\n--- Step 6: Bob receives remaining segments (26000, 27000, 28000) -> Sends DupACKs ---"
    );
    receive_and_ack_with_dups(
        &mut bob,
        alice.address,
        &mut link,
        &mut bobs_expected_seq,
        &mut Some(&mut logger),
    );

    // 8. Alice processes DupACK #3, #4, #5 -> Fast Retransmit Seg 1 (21000) & Inflates cwnd
    println!("\n--- Step 7: Alice processes DupACKs #3, #4, #5 -> Triggers Fast Retransmit ---");
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    // 9. Bob receives retransmitted 21000 -> Partial ACK 22000
    println!("\n--- Step 8: Bob receives retransmitted 21000 -> Sends Partial ACK (22000) ---");
    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 21000 {
                bobs_expected_seq = 22000;
                println!("--> Received 21000! Sending Partial ACK for 22000");
                logger.log_send_ack(22000, false);
                bob.send(Packet::new_ack(22000), alice.address, &mut link, &mut None);
            }
        }
    }

    // 10. Alice processes Partial ACK 22000 -> Retransmits Seg 2 (22000)
    println!("\n--- Step 9: Alice processes Partial ACK (22000) & retransmits Seg 2 (22000) ---");
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    // 11. Bob receives 22000 -> Partial ACK 24000
    println!("\n--- Step 10: Bob receives 22000 -> Sends Partial ACK (24000) ---");
    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 22000 {
                bobs_expected_seq = 24000;
                println!("--> Received 22000! Sending Partial ACK for 24000");
                logger.log_send_ack(24000, false);
                bob.send(Packet::new_ack(24000), alice.address, &mut link, &mut None);
            }
        }
    }

    // 12. Alice processes Partial ACK 24000 -> Retransmits Seg 4 (24000)
    println!("\n--- Step 11: Alice processes Partial ACK (24000) & retransmits Seg 4 (24000) ---");
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));
    alice.print();

    // 13. Bob receives 24000 -> Full ACK 29000
    println!("\n--- Step 12: Bob receives 24000 -> Sends FULL ACK (29000) ---");
    while let Some(packet) = bob.receive(&mut link, alice.address, &mut None) {
        if let Packet::Data { seq, .. } = packet {
            if seq == 24000 {
                bobs_expected_seq = 29000;
                println!(
                    "--> Received 24000! Buffer now complete through 28000. Sending FULL ACK for 29000"
                );
                logger.log_send_ack(29000, false);
                bob.send(Packet::new_ack(29000), alice.address, &mut link, &mut None);
            }
        }
    }

    // 14. Alice processes FULL ACK 29000 -> Exits Fast Recovery
    println!("\n--- Step 13: Alice processes Full ACK (29000) & Exits Fast Recovery ---");
    drain_channel(&mut alice, &mut link, bob.address, &mut Some(&mut logger));

    println!("Bob's expected seq: {}", bobs_expected_seq);
    println!("\n--- FINAL POST-RECOVERY STATE ---");
    alice.print();

    logger.finish(filename);
}

fn main() {
    let args = Args::parse();

    if args.exercise_1 {
        // ... (run_exercise_1)
    } else if args.exercise_2 {
        // ... (run_exercise_2)
    } else if args.exercise_3 {
        run_exercise_3();
    } else {
        println!("No exercise flag provided.");
        println!("Run with `--exercise-1`, `--exercise-2`, or `--exercise-3`.");
    }
}
