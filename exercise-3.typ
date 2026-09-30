#import "@preview/chronos:0.3.0": *
#set text(font: ("Liberation Sans", "DejaVu Sans", "Arial"))

#align(center)[
  #diagram({
    _par("S", display-name: [Sender])
    _par("R", display-name: [Receiver])

    _seq("S", "R", comment: [Seg. 21000 \ (Initial State: cwnd=6MSS, ssthresh=64MSS, dupACKs=0, recover=0)])
    _seq("S", "R", comment: [Seg. 22000])
    _seq("S", "R", comment: [Seg. 23000])
    _seq("S", "R", comment: [Seg. 24000])
    _seq("S", "R", comment: [Seg. 25000])
    _seq("S", "R", comment: [Seg. 26000])
    _seq("S", "R", comment: [Seg. 21000 Dropped])
    _seq("S", "R", comment: [Seg. 22000 Dropped])
    _seq("S", "R", comment: [Seg. 24000 Dropped])
    _seq("R", "S", comment: [DupACK 21000])
    _seq("R", "S", comment: [DupACK 21000])
    _seq("S", "R", comment: [Seg. 27000])
    _seq("R", "S", comment: [DupACK 21000])
    _seq("R", "S", comment: [DupACK 21000])
    _seq("S", "R", comment: [Seg. 28000])
    _seq("S", "R", comment: [Seg. 22000 \ (Fast Retransmit: cwnd=6MSS, ssthresh=3MSS, dupACKs=3, recover=28000)])
  })
]
