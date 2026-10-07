#set page(width: auto, height: auto, margin: 1cm)
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
    _seq("R", "S", comment: [DupACK 21000 SACK[22000-23000]])
    _seq("R", "S", comment: [DupACK 21000 SACK[22000-24000]])
    _seq("R", "S", comment: [DupACK 21000 SACK[22000-25000]])
    _seq("R", "S", comment: [DupACK 21000 SACK[27000-28000, 22000-25000]])
    _seq("R", "S", comment: [DupACK 21000 SACK[27000-29000, 22000-25000]])
    _seq("S", "R", comment: [Seg. 21000 \ (SACK Recovery \ cwnd=4MSS, ssthresh=4MSS, dupACKs=3 \ pipe=4, recover=28000 \ sacked={22000, 23000, 24000})])
    _seq("S", "R", comment: [Seg. 25000 \ (SACK Recovery \ cwnd=4MSS, ssthresh=4MSS, dupACKs=5 \ pipe=3, recover=28000 \ sacked={22000, 23000, 24000, 27000, 28000})])
    _seq("R", "S", comment: [Ack. 25000 SACK[27000-29000]])
    _seq("R", "S", comment: [Ack. 26000 SACK[27000-29000]])
    _seq("S", "R", comment: [Seg. 26000 \ (SACK Recovery \ cwnd=4MSS, ssthresh=4MSS, dupACKs=5 \ pipe=3, recover=28000 \ sacked={27000, 28000})])
    _seq("R", "S", comment: [Ack. 29000 \ (SACK Recovery \ cwnd=4MSS, ssthresh=4MSS, dupACKs=5 \ pipe=2, recover=28000 \ sacked={27000, 28000})])
    _seq("S", "R", comment: [Final State: Exit SACK Recovery \ cwnd=4MSS, ssthresh=4MSS, dupACKs=0 \ pipe=0, recover=28000 \ sacked={}])
  })
]
