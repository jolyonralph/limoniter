; hardware.i - the Amiga's custom chips and CIAs, by name.
;
; How To Code v8. CC0: use it for anything.
;
; The custom chip registers are offsets from CUSTOM ($DFF000); keep CUSTOM in an address register (a5 in these
; examples) and write move.w #$8380,DMACON(a5). The names are the Hardware Reference Manual's, and the NDK's.

CUSTOM          equ     $dff000

; ---- Read-only registers
BLTDDAT         equ     $000
DMACONR         equ     $002
VPOSR           equ     $004
VHPOSR          equ     $006
JOY0DAT         equ     $00a
JOY1DAT         equ     $00c
CLXDAT          equ     $00e
ADKCONR         equ     $010
POT0DAT         equ     $012
POT1DAT         equ     $014
POTGOR          equ     $016
SERDATR         equ     $018
DSKBYTR         equ     $01a
INTENAR         equ     $01c
INTREQR         equ     $01e
DENISEID        equ     $07c            ; ECS and AGA: which Denise or Lisa (OCS: whatever's on the bus)

; ---- The blitter
BLTCON0         equ     $040
BLTCON1         equ     $042
BLTAFWM         equ     $044
BLTALWM         equ     $046
BLTCPTH         equ     $048
BLTCPTL         equ     $04a
BLTBPTH         equ     $04c
BLTBPTL         equ     $04e
BLTAPTH         equ     $050
BLTAPTL         equ     $052
BLTDPTH         equ     $054
BLTDPTL         equ     $056
BLTSIZE         equ     $058
BLTCON0L        equ     $05a            ; ECS
BLTSIZV         equ     $05c            ; ECS
BLTSIZH         equ     $05e            ; ECS
BLTCMOD         equ     $060
BLTBMOD         equ     $062
BLTAMOD         equ     $064
BLTDMOD         equ     $066
BLTCDAT         equ     $070
BLTBDAT         equ     $072
BLTADAT         equ     $074

; ---- The copper
COPCON          equ     $02e
COP1LCH         equ     $080
COP1LC          equ     $080
COP1LCL         equ     $082
COP2LCH         equ     $084
COP2LC          equ     $084
COP2LCL         equ     $086
COPJMP1         equ     $088
COPJMP2         equ     $08a

; ---- The display
DIWSTRT         equ     $08e
DIWSTOP         equ     $090
DDFSTRT         equ     $092
DDFSTOP         equ     $094
DMACON          equ     $096
CLXCON          equ     $098
INTENA          equ     $09a
INTREQ          equ     $09c
ADKCON          equ     $09e
BPL1PTH         equ     $0e0
BPL1PTL         equ     $0e2
BPL2PTH         equ     $0e4
BPL2PTL         equ     $0e6
BPL3PTH         equ     $0e8
BPL3PTL         equ     $0ea
BPL4PTH         equ     $0ec
BPL4PTL         equ     $0ee
BPL5PTH         equ     $0f0
BPL5PTL         equ     $0f2
BPL6PTH         equ     $0f4
BPL6PTL         equ     $0f6
BPL7PTH         equ     $0f8            ; AGA
BPL7PTL         equ     $0fa
BPL8PTH         equ     $0fc            ; AGA
BPL8PTL         equ     $0fe
BPLCON0         equ     $100
BPLCON1         equ     $102
BPLCON2         equ     $104
BPLCON3         equ     $106            ; ECS and AGA
BPL1MOD         equ     $108
BPL2MOD         equ     $10a
BPLCON4         equ     $10c            ; AGA
BPL1DAT         equ     $110
SPR0PTH         equ     $120
SPR0PTL         equ     $122
SPR0POS         equ     $140
SPR0CTL         equ     $142
SPR0DATA        equ     $144
SPR0DATB        equ     $146
COLOR00         equ     $180
COLOR01         equ     $182
COLOR02         equ     $184
COLOR03         equ     $186
COLOR04         equ     $188
COLOR05         equ     $18a
COLOR06         equ     $18c
COLOR07         equ     $18e
COLOR08         equ     $190
COLOR09         equ     $192
COLOR10         equ     $194
COLOR11         equ     $196
COLOR12         equ     $198
COLOR13         equ     $19a
COLOR14         equ     $19c
COLOR15         equ     $19e
COLOR16         equ     $1a0
COLOR17         equ     $1a2
COLOR18         equ     $1a4
COLOR19         equ     $1a6
COLOR20         equ     $1a8
COLOR21         equ     $1aa
COLOR22         equ     $1ac
COLOR23         equ     $1ae
COLOR24         equ     $1b0
COLOR25         equ     $1b2
COLOR26         equ     $1b4
COLOR27         equ     $1b6
COLOR28         equ     $1b8
COLOR29         equ     $1ba
COLOR30         equ     $1bc
COLOR31         equ     $1be
BEAMCON0        equ     $1dc            ; ECS and AGA
FMODE           equ     $1fc            ; AGA

; ---- Audio
AUD0LCH         equ     $0a0
AUD0LC          equ     $0a0
AUD0LEN         equ     $0a4
AUD0PER         equ     $0a6
AUD0VOL         equ     $0a8
AUD0DAT         equ     $0aa
AUD1LC          equ     $0b0
AUD2LC          equ     $0c0
AUD3LC          equ     $0d0
AUD_SIZE        equ     $010            ; each channel's registers are this far after the one before

; ---- DMACON's bits (and DMACONR's)
DMAF_SETCLR     equ     $8000           ; write: set the bits given, rather than clear them
DMAF_BLTDONE    equ     $4000           ; read: the blitter is busy (BBUSY)
DMAF_BLTNZERO   equ     $2000           ; read: the last blit's result was all zero
DMAF_BLITHOG    equ     $0400           ; the blitter takes the bus from the processor
DMAF_MASTER     equ     $0200           ; DMAEN: no DMA at all without it
DMAF_RASTER     equ     $0100           ; bitplanes
DMAF_COPPER     equ     $0080
DMAF_BLITTER    equ     $0040
DMAF_SPRITE     equ     $0020
DMAF_DISK       equ     $0010
DMAF_AUD3       equ     $0008
DMAF_AUD2       equ     $0004
DMAF_AUD1       equ     $0002
DMAF_AUD0       equ     $0001
DMAF_AUDIO      equ     $000f
DMAB_BLTDONE    equ     14

; ---- INTENA's and INTREQ's bits
INTF_SETCLR     equ     $8000
INTF_INTEN      equ     $4000           ; the master switch: no interrupts at all without it
INTF_EXTER      equ     $2000           ; level 6: CIA-B and the expansion bus
INTF_DSKSYNC    equ     $1000
INTF_RBF        equ     $0800
INTF_AUD3       equ     $0400
INTF_AUD2       equ     $0200
INTF_AUD1       equ     $0100
INTF_AUD0       equ     $0080
INTF_BLIT       equ     $0040           ; level 3: the blitter has finished
INTF_VERTB      equ     $0020           ; level 3: the start of a frame
INTF_COPER      equ     $0010           ; level 3: the copper asked for it
INTF_PORTS      equ     $0008           ; level 2: CIA-A and the expansion bus
INTF_SOFTINT    equ     $0004
INTF_DSKBLK     equ     $0002
INTF_TBE        equ     $0001
INTB_VERTB      equ     5
INTB_COPER      equ     4
INTB_BLIT       equ     6
INTB_PORTS      equ     3
INTB_EXTER      equ     13

; ---- The CIAs. CIA-A is on the odd bytes ($BFExx1), CIA-B on the even ($BFDxx0); each register is $100 on.
CIAA            equ     $bfe001
CIAB            equ     $bfd000
CIAPRA          equ     $0000
CIAPRB          equ     $0100
CIADDRA         equ     $0200
CIADDRB         equ     $0300
CIATALO         equ     $0400
CIATAHI         equ     $0500
CIATBLO         equ     $0600
CIATBHI         equ     $0700
CIATODLOW       equ     $0800
CIATODMID       equ     $0900
CIATODHI        equ     $0a00
CIASDR          equ     $0c00
CIAICR          equ     $0d00
CIACRA          equ     $0e00
CIACRB          equ     $0f00
; CIAICR's bits (reading tells which happened, and clears them; writing with bit 7 set enables them)
CIAICRB_TA      equ     0
CIAICRB_TB      equ     1
CIAICRB_ALRM    equ     2
CIAICRB_SP      equ     3
CIAICRB_FLG     equ     4
CIAICRB_IR      equ     7               ; read: an interrupt happened
CIAICRB_SETCLR  equ     7               ; write: set the bits given, rather than clear them
; CIACRA's and CIACRB's bits
CIACRAB_START   equ     0
CIACRAB_PBON    equ     1
CIACRAB_OUTMODE equ     2
CIACRAB_RUNMODE equ     3               ; 1: one shot
CIACRAB_LOAD    equ     4               ; write 1: load the latch into the timer now
CIACRAB_INMODE  equ     5
CIACRAB_SPMODE  equ     6               ; CIA-A: the serial port is output (the keyboard handshake)
CIACRAB_TODIN   equ     7
CIACRBB_START   equ     0
CIACRBB_PBON    equ     1
CIACRBB_OUTMODE equ     2
CIACRBB_RUNMODE equ     3
CIACRBB_LOAD    equ     4
CIACRBB_INMODE0 equ     5
CIACRBB_INMODE1 equ     6
CIACRBB_ALARM   equ     7
CIAB_GAMEPORT0  equ     6               ; CIA-A PRA: 0 while the left mouse button (port 0's fire) is down
CIAB_GAMEPORT1  equ     7               ; CIA-A PRA: 0 while the joystick's fire button (port 1) is down
