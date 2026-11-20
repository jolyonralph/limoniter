; ch08_dualpf.s - dual playfield parallax: two playfields of two bitplanes each, one in front of the other, each
; with its own bitplanes, modulo and scroll. The hills behind move a pixel a frame, the pillars in front two.
; Each picture repeats every 320 pixels, so both scroll on for ever. Left mouse button to leave.
;
; How To Code v8, chapter 8. CC0: use it for anything.
;
; $VER: ch08_dualpf 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

PERIOD          equ     320             ; both pictures repeat after this many pixels
PICWIDTH        equ     16+PERIOD+320   ; an empty word, then enough to show any 320 pixels of a period
ROWBYTES        equ     PICWIDTH/8
HEIGHT          equ     256
PLANESIZE       equ     ROWBYTES*HEIGHT
FETCHBYTES      equ     42              ; 21 words a line: 20 shown, one more for scrolling
GROUND          equ     236             ; the front's ground starts on this row

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: the two pictures.

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2        ; NTSC: 200 lines
.pal:
        bsr     DrawHills
        bsr     DrawPillars
        bsr     VBlank                  ; the pointers for the first frame
        rts

; The back playfield (bitplanes 2 and 4): hills from two sine waves, 1 and 3 waves to a period, in three bands
; of colour (playfield 2's colours 1 to 3 are COLOR09 to COLOR11).
DrawHills:
        lea     Sine,a2
        moveq   #0,d7                   ; x
.column:
        move.w  d7,d0
        bsr     PeriodAngle             ; x as an angle: 256 to a period
        move.w  d0,d2
        add.w   d0,d0
        move.w  (a2,d0.w),d1
        muls    #36,d1
        move.w  d2,d0
        mulu    #3,d0
        add.w   #50,d0
        and.w   #255,d0
        add.w   d0,d0
        move.w  (a2,d0.w),d0
        muls    #14,d0
        add.l   d0,d1
        add.l   d1,d1
        add.l   d1,d1
        swap    d1
        add.w   #130,d1                 ; the top of the hill here
        move.w  d1,d6
        move.w  d6,d3
        lea     BackPlanes,a0
        bsr     ColumnStart
        moveq   #1,d4                   ; colour 1, then 2 after 12 lines, then 3 after 30
.pixel:
        bsr     SetPixel
        move.w  d3,d0
        sub.w   d6,d0
        cmp.w   #12,d0
        bne.s   .notband2
        moveq   #2,d4
.notband2:
        cmp.w   #30,d0
        bne.s   .notband3
        moveq   #3,d4
.notband3:
        addq.w  #1,d3
        cmp.w   #HEIGHT,d3
        blo.s   .pixel
        addq.w  #1,d7
        cmp.w   #PICWIDTH-16,d7
        blo.s   .column
        rts

; The front playfield (bitplanes 1 and 3): a pillar every 80 pixels, lit from the left, and the ground.
DrawPillars:
        moveq   #0,d7
.column:
        lea     FrontPlanes,a0
        move.w  #GROUND,d6
        moveq   #1,d4                   ; the ground: colour 1 all the way down
        bsr     ColumnFill
        move.w  d7,d0
        ext.l   d0
        divu    #80,d0
        move.w  d0,d5                   ; which pillar, counting from the left
        swap    d0                      ; how far across it
        cmp.w   #20,d0
        bhs.s   .next
        moveq   #2,d4                   ; the pillar's face
        tst.w   d0
        beq.s   .light
        cmp.w   #1,d0
        bne.s   .notlight
.light:
        moveq   #3,d4                   ; its lit edge
.notlight:
        cmp.w   #17,d0
        blo.s   .notshade
        moveq   #1,d4                   ; its shaded edge
.notshade:
        and.w   #3,d5                   ; four heights, repeating every 320 pixels
        add.w   d5,d5
        lea     PillarTops,a1
        move.w  (a1,d5.w),d6
        lea     FrontPlanes,a0
        move.w  d6,d3
        bsr     ColumnStart
.pixel:
        bsr     SetPixel
        addq.w  #1,d3
        cmp.w   #GROUND,d3
        blo.s   .pixel
.next:
        addq.w  #1,d7
        cmp.w   #PICWIDTH-16,d7
        blo.s   .column
        rts

; Colour d4 from row d6 down to the bottom, in column d7 of the playfield at a0.
ColumnFill:
        move.w  d6,d3
        bsr     ColumnStart
.pixel:
        bsr     SetPixel
        addq.w  #1,d3
        cmp.w   #HEIGHT,d3
        blo.s   .pixel
        rts

; The byte of column d7, row d3, in the playfield's two bitplanes at a0: a3 and a4 point at it, d1 is the bit.
ColumnStart:
        move.w  d3,d0
        mulu    #ROWBYTES,d0
        move.w  d7,d1
        add.w   #16,d1                  ; past the empty word on the left
        lsr.w   #3,d1
        add.w   d1,d0
        move.l  a0,a3
        add.l   d0,a3
        move.l  a3,a4
        add.l   #PLANESIZE,a4
        move.w  d7,d1
        not.w   d1                      ; the bit: 7 - (x and 7)
        rts

; Colour d4 (0 to 3) at a3, a4, bit d1; then a3 and a4 a row down.
SetPixel:
        btst    #0,d4
        beq.s   .low
        bset    d1,(a3)
.low:
        btst    #1,d4
        beq.s   .high
        bset    d1,(a4)
.high:
        add.w   #ROWBYTES,a3
        add.w   #ROWBYTES,a4
        rts

; x (d0) as an angle, 256 to a period, into d0.
PeriodAngle:
        ext.l   d0
        lsl.l   #8,d0
        divu    #PERIOD,d0
        and.w   #255,d0
        rts

; ---------------------------------------------------------------------------------------------------------------

Frame:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of each frame: each playfield's pointers and delay, as in ch08_scroll.s. The copper sets them
; after its WAIT for line 16. BPLCON1 holds both delays: playfield 1's in bits 0-3, playfield 2's in bits 4-7.

VBlank:
        addq.w  #1,Position
        move.w  Position,d0
        ext.l   d0
        divu    #PERIOD,d0
        swap    d0                      ; the back: a pixel a frame, round the period
        lea     BackPlanes,a0
        lea     CopperBitplanes+8,a1    ; bitplanes 2 and 4
        bsr     .playfield
        lsl.w   #4,d2
        move.w  d2,d3
        move.w  Position,d0
        add.w   d0,d0
        ext.l   d0
        divu    #PERIOD,d0
        swap    d0                      ; the front: two pixels a frame
        lea     FrontPlanes,a0
        lea     CopperBitplanes,a1      ; bitplanes 1 and 3
        bsr     .playfield
        or.w    d3,d2
        move.w  d2,CopperScroll+2
        rts

; Shows the playfield at a0 from pixel d0: its two bitplanes' pointers at a1 (16 bytes apart in the list); the
; delay is returned in d2.
.playfield:
        move.w  d0,d1
        add.w   #15,d1
        lsr.w   #4,d1                   ; the word to fetch from
        move.w  d1,d2
        lsl.w   #4,d2
        sub.w   d0,d2                   ; the delay
        add.w   d1,d1
        ext.l   d1
        add.l   a0,d1
        moveq   #2-1,d0
.plane:
        move.w  d1,6(a1)
        swap    d1
        move.w  d1,2(a1)
        swap    d1
        add.l   #PLANESIZE,d1
        add.w   #16,a1                  ; this playfield's next bitplane: two on in the list
        dbf     d0,.plane
        rts

        section data,data

Position:       dc.w    0
PillarTops:     dc.w    90,140,60,120

; Sine, 256 steps to a full turn, from -16384 to 16384. Made by this line of Python (or with Limoniter's
; Tools > General > Table generator):
;   [round(16384*math.sin(i*math.pi/128)) for i in range(256)]
Sine:
        dc.w    0,402,804,1205,1606,2006,2404,2801,3196,3590,3981,4370,4756,5139,5520,5897
        dc.w    6270,6639,7005,7366,7723,8076,8423,8765,9102,9434,9760,10080,10394,10702,11003,11297
        dc.w    11585,11866,12140,12406,12665,12916,13160,13395,13623,13842,14053,14256,14449,14635,14811,14978
        dc.w    15137,15286,15426,15557,15679,15791,15893,15986,16069,16143,16207,16261,16305,16340,16364,16379
        dc.w    16384,16379,16364,16340,16305,16261,16207,16143,16069,15986,15893,15791,15679,15557,15426,15286
        dc.w    15137,14978,14811,14635,14449,14256,14053,13842,13623,13395,13160,12916,12665,12406,12140,11866
        dc.w    11585,11297,11003,10702,10394,10080,9760,9434,9102,8765,8423,8076,7723,7366,7005,6639
        dc.w    6270,5897,5520,5139,4756,4370,3981,3590,3196,2801,2404,2006,1606,1205,804,402
        dc.w    0,-402,-804,-1205,-1606,-2006,-2404,-2801,-3196,-3590,-3981,-4370,-4756,-5139,-5520,-5897
        dc.w    -6270,-6639,-7005,-7366,-7723,-8076,-8423,-8765,-9102,-9434,-9760,-10080,-10394,-10702,-11003,-11297
        dc.w    -11585,-11866,-12140,-12406,-12665,-12916,-13160,-13395,-13623,-13842,-14053,-14256,-14449,-14635,-14811,-14978
        dc.w    -15137,-15286,-15426,-15557,-15679,-15791,-15893,-15986,-16069,-16143,-16207,-16261,-16305,-16340,-16364,-16379
        dc.w    -16384,-16379,-16364,-16340,-16305,-16261,-16207,-16143,-16069,-15986,-15893,-15791,-15679,-15557,-15426,-15286
        dc.w    -15137,-14978,-14811,-14635,-14449,-14256,-14053,-13842,-13623,-13395,-13160,-12916,-12665,-12406,-12140,-11866
        dc.w    -11585,-11297,-11003,-10702,-10394,-10080,-9760,-9434,-9102,-8765,-8423,-8076,-7723,-7366,-7005,-6639
        dc.w    -6270,-5897,-5520,-5139,-4756,-4370,-3981,-3590,-3196,-2801,-2404,-2006,-1606,-1205,-804,-402

; ---------------------------------------------------------------------------------------------------------------

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00           ; AGA: playfield 2's colours start at 8, as on OCS and ECS
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0030           ; 16 pixels early, for scrolling
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$4600           ; four bitplanes, dual playfield (bit 10), colour
        dc.w    BPLCON2,$0000           ; playfield 1 in front (bit 6, PF2PRI, would put 2 in front)
        dc.w    BPL1MOD,ROWBYTES-FETCHBYTES     ; the odd bitplanes: playfield 1
        dc.w    BPL2MOD,ROWBYTES-FETCHBYTES     ; the even bitplanes: playfield 2
        dc.w    COLOR00,$0439           ; the sky's top colour, for the border above the window
        dc.w    COLOR01,$0420           ; playfield 1: the ground and the pillars' shade, face, light
        dc.w    COLOR02,$0a74
        dc.w    COLOR03,$0fdb
        dc.w    COLOR09,$0759           ; playfield 2: the hills, from their tops down
        dc.w    COLOR10,$0547
        dc.w    COLOR11,$0335
        dc.w    $1007,$fffe             ; line 16: VBlank has set what follows by now
CopperScroll:
        dc.w    BPLCON1,$0000
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0

        ; The sky behind both: a band of colour every 16 lines.
LINE    set     $2c
BAND    set     0
        rept    12
        dc.w    (LINE<<8)!$07,$fffe
        dc.w    COLOR00,((4+BAND*11/12)<<8)!((3+BAND/2)<<4)!(9-BAND/3)
LINE    set     LINE+16
BAND    set     BAND+1
        endr
        dc.w    $ffff,$fffe

        section screen,bss_c

FrontPlanes:    ds.b    PLANESIZE*2
BackPlanes:     ds.b    PLANESIZE*2
