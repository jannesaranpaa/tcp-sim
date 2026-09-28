#import "@preview/chronos:0.3.0": *
#set text(font: ("Liberation Sans", "DejaVu Sans", "Arial"))

#align(center)[
  #diagram({
    _par("S", display-name: [Sender])
    _par("R", display-name: [Receiver])

    _seq("S", "R", comment: [Seg. 21000 \ (Initial State: cwnd=8MSS, ssthresh=64MSS, dupACKs=0, recover=0)])
    _seq("S", "R", comment: [Seg. 22000])
    _seq("S", "R", comment: [Seg. 23000])
    _seq("S", "R", comment: [Seg. 24000])
    _seq("S", "R", comment: [Seg. 25000])
    _seq("S", "R", comment: [Seg. 26000])
    _seq("S", "R", comment: [Seg. 27000])
    _seq("S", "R", comment: [Seg. 28000])
    _seq("S", "R", comment: [Seg. 21000 Dropped])
    _seq("S", "R", comment: [Seg. 25000 Dropped])
    _seq("S", "R", comment: [Seg. 26000 Dropped])
    _seq("R", "S", comment: [DupACK 20000])
    _seq("R", "S", comment: [DupACK 20000])
    _seq("R", "S", comment: [DupACK 20000])
    _seq("R", "S", comment: [DupACK 20000])
    _seq("R", "S", comment: [DupACK 20000])
    _seq("S", "R", comment: [Seg. 21000 \ (Fast Retransmit: cwnd=7MSS, ssthresh=4MSS, dupACKs=3, recover=28000)])
    _seq("R", "S", comment: [Ack. 24000 \ (DupACK (Inflate cwnd): cwnd=9MSS, ssthresh=4MSS, dupACKs=5, recover=28000)])
    _seq("S", "R", comment: [Seg. 25000 \ (Partial ACK: cwnd=5MSS, ssthresh=4MSS, dupACKs=5, recover=28000)])
    _seq("R", "S", comment: [Ack. 25000])
    _seq("S", "R", comment: [Seg. 26000 \ (Partial ACK: cwnd=4MSS, ssthresh=4MSS, dupACKs=5, recover=28000)])
    _seq("R", "S", comment: [Ack. 28000])
    _seq("S", "R", comment: [Final State: Exit Fast Recovery: cwnd=4MSS, ssthresh=4MSS, dupACKs=0, recover=28000])
  })
]
