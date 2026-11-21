; ch17_random.s - random numbers and fixed-point maths. Above: fireworks, each spark a position and a speed in
; 16.16 fixed point (a pixel and its fraction in a long), pulled down by gravity and slowed by the air, its burst
; and its direction picked at random. Below, two boxes filled with dots at "random" places, a few a frame: on the
; left the places come from the beam's position (VHPOSR), as people often do, and fall into lines and gaps; on the
; right they come from xorshift, and fill the box evenly. Left mouse button to leave.
;
; How To Code v8, chapter 17. CC0: use it for anything.
;
; $VER: ch17_random 8.0 (21.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

SKYLINES        equ     192             ; the fireworks' part of the screen
SKYBYTES        equ     40*SKYLINES
DOTLINES        equ     64              ; the boxes' part, under it
SPARKS          equ     144
BURST           equ     48              ; sparks in a burst
GRAVITY         equ     2600            ; 0.04 of a pixel a frame, every frame, in 16.16
BOXWIDTH        equ     128
BOXLINES        equ     56
DOTS            equ     16              ; dots a frame in each box

        section code,code

Init:
        move.l  #NoSprite,d0
        lea     CopperSprites,a0
        moveq   #8-1,d1
.sprite:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        dbf     d1,.sprite
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2 ; NTSC: 200 lines, so the boxes are cut short
.pal:
        ; Everything starts empty (BSS memory isn't cleared on every Kickstart).
        lea     SkyA,a0
        move.w  #(SKYBYTES*2+40*DOTLINES)/4-1,d0
.clear:
        clr.l   (a0)+
        dbf     d0,.clear
        ; The boxes' frames.
        lea     Dots,a0
        moveq   #DOTLINES-1,d1
.line:
        move.b  #$01,1(a0)              ; the left box's left edge: the pixel before x 16
        move.b  #$80,18(a0)             ; its right edge: x 144
        move.b  #$01,21(a0)             ; the right box's: x 175...
        move.b  #$80,38(a0)             ; ...and 304
        add.w   #40,a0
        dbf     d1,.line
        move.l  #Dots,d0
        move.w  d0,CopperDots+6
        swap    d0
        move.w  d0,CopperDots+2
        move.l  #SkyA,d0
        bra     ShowSky

; ---------------------------------------------------------------------------------------------------------------

Frame:
        ; Clear the sky that isn't shown, with the blitter, while the processor gets on with the dots.
        BLIT_BEGIN
        move.l  DrawSky,BLTDPTH(a5)
        move.w  #0,BLTDMOD(a5)
        move.l  #$01000000,BLTCON0(a5)  ; D only, all zeros
        move.w  #(SKYLINES<<6)!20,BLTSIZE(a5)

        bsr     BeamDots
        bsr     XorshiftDots

        BLIT_END                        ; the sky must be clear before the sparks are drawn in it

        ; A new burst every 40 frames.
        subq.w  #1,NextBurst
        bgt.s   .noburst
        move.w  #40,NextBurst
        bsr     Burst
.noburst:
        bsr     MoveSparks

        move.l  DrawSky,d0
        move.l  ShownSky,DrawSky
        move.l  d0,ShownSky

; The sky at d0 into the copper list.
ShowSky:
        move.w  d0,CopperSky+6
        swap    d0
        move.w  d0,CopperSky+2
        rts

; ---------------------------------------------------------------------------------------------------------------
; A burst: BURST sparks, at a random place, each going off in a random direction at a random speed.

Burst:
        bsr     Random
        and.l   #127,d0
        add.w   #96,d0
        swap    d0                      ; the place, in 16.16: a pixel from 96 to 223...
        move.l  d0,d6
        bsr     Random
        and.l   #63,d0
        add.w   #40,d0
        swap    d0
        move.l  d0,d5                   ; ...and a line from 40 to 103
        lea     Sparks,a0
        move.w  #SPARKS-1,d7            ; (more than MOVEQ can take)
        moveq   #BURST-1,d4
.find:
        tst.w   16(a0)                  ; a spark that's burnt out
        bgt.s   .busy
.again:
        ; A speed in each direction from -2 to just under 2 pixels a frame: a random long, shifted down with its
        ; sign, is a random 16.16 number between -2 and 2. Kept only if it's inside a circle of radius 2, so the
        ; burst is round, not square: picking again when it isn't ("rejection") keeps it even.
        bsr     Random
        asr.l   #8,d0
        asr.l   #6,d0
        move.l  d0,d2                   ; x speed
        bsr     Random
        asr.l   #8,d0
        asr.l   #6,d0
        move.l  d0,d3                   ; y speed
        move.l  d2,d0
        asr.l   #8,d0                   ; to 8.8, so it fits a word to square
        muls    d0,d0
        move.l  d3,d1
        asr.l   #8,d1
        muls    d1,d1
        add.l   d1,d0
        cmp.l   #(2<<8)*(2<<8),d0
        bhi.s   .again
        movem.l d2-d3/d5-d6,-(sp)
        move.l  d6,(a0)                 ; x
        move.l  d5,4(a0)                ; y
        move.l  d2,8(a0)
        move.l  d3,12(a0)
        bsr     Random
        and.w   #31,d0
        add.w   #50,d0
        move.w  d0,16(a0)               ; frames to live
        movem.l (sp)+,d2-d3/d5-d6
        subq.w  #1,d4
        bmi.s   .done
.busy:
        lea     SPARKBYTES(a0),a0
        dbf     d7,.find
.done:
        rts

SPARKBYTES      equ     18

; Each spark: x += x speed, y += y speed, the y speed pulled down by gravity, both slowed a little by the air.
; In 16.16, the top word is the pixel and the bottom word its fraction: SWAP gives the pixel to draw at.
MoveSparks:
        lea     Sparks,a0
        move.l  DrawSky,a1
        move.w  #SPARKS-1,d7            ; (more than MOVEQ can take)
.spark:
        tst.w   16(a0)
        ble.s   .next
        subq.w  #1,16(a0)
        movem.l (a0),d0-d3              ; x, y, x speed, y speed: all 16.16
        add.l   #GRAVITY,d3             ; 2600: 0.04 of a pixel, as 2600/65536
        move.l  d2,d4
        asr.l   #6,d4
        sub.l   d4,d2                   ; the air: 1/64 of the speed lost each frame
        move.l  d3,d4
        asr.l   #6,d4
        sub.l   d4,d3
        add.l   d2,d0
        add.l   d3,d1
        movem.l d0-d3,(a0)
        swap    d0                      ; the pixel
        swap    d1                      ; the line
        cmp.w   #320,d0
        bhs.s   .next                   ; (unsigned: off the left is a large number too)
        cmp.w   #SKYLINES-1,d1
        bhs.s   .next
        mulu    #40,d1
        move.w  d0,d4
        lsr.w   #3,d4
        add.w   d4,d1
        not.b   d0                      ; the bit: 7 - (x and 7), and BSET on memory uses only the low 3 bits
        bset    d0,(a1,d1.l)
        bset    d0,40(a1,d1.l)          ; two lines tall
.next:
        lea     SPARKBYTES(a0),a0
        dbf     d7,.spark
        rts

; ---------------------------------------------------------------------------------------------------------------
; The mistake: the beam's position as a random number. Each read comes a fixed time after the last, so the
; numbers step along together, and the dots fall into lines.
BeamDots:
        moveq   #DOTS-1,d7
.dot:
        move.w  VHPOSR(a5),d0           ; x from one read...
        move.w  VHPOSR(a5),d1           ; ...and y from the next
        and.w   #BOXWIDTH-1,d0
        add.w   #16,d0
        and.w   #63,d1
        bsr     Dot
        dbf     d7,.dot
        rts

; The fix: a proper generator.
XorshiftDots:
        moveq   #DOTS-1,d7
.dot:
        bsr     Random
        move.w  d0,d1
        lsr.w   #8,d1
        and.w   #BOXWIDTH-1,d0
        add.w   #176,d0
        and.w   #63,d1
        bsr     Dot
        dbf     d7,.dot
        rts

; A dot at d0, d1 in the boxes, if it's inside one: d1 from 0 to 63, only 0 to 55 are drawn. Taking the low bits
; for a range that isn't a power of two (y MOD 56) would make some lines come up more than others; dropping the
; numbers past the end (as here) keeps every line as likely as the rest.
Dot:
        cmp.w   #BOXLINES,d1
        bhs.s   .out
        addq.w  #4,d1                   ; under the top edge
        mulu    #40,d1
        move.w  d0,d2
        lsr.w   #3,d2
        add.w   d2,d1
        lea     Dots,a0
        not.b   d0
        bset    d0,(a0,d1.l)
.out:
        rts

; Xorshift32 (George Marsaglia, 2003): three shifts and three EORs make the next number from the last. It goes
; through every 32-bit number but 0 before it repeats, and 0 must never be the seed. Returns d0.l; changes d1.
Random:
        move.l  Seed,d0
        move.l  d0,d1
        lsl.l   #8,d1
        lsl.l   #5,d1
        eor.l   d1,d0
        move.l  d0,d1
        swap    d1                      ; >> 17: a SWAP is >> 16...
        lsr.w   #1,d1                   ; ...and one more, with the top word...
        and.l   #$7fff,d1               ; ...cleared
        eor.l   d1,d0
        move.l  d0,d1
        lsl.l   #5,d1
        eor.l   d1,d0
        move.l  d0,Seed
        rts

VBlank:
        rts

        section data,data

Seed:           dc.l    $2545f491       ; a game would mix in the time of a key press (chapter 17)
DrawSky:        dc.l    SkyB
ShownSky:       dc.l    SkyA
NextBurst:      dc.w    1
Sparks:         ds.b    SPARKBYTES*SPARKS ; each: x, y, x speed, y speed (16.16 longs), and frames to live

        section copper,data_c

Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    COLOR00,$0002
CopperSky:
        dc.w    BPL1PTH,0,BPL1PTL,0
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        ; The sparks' colour changes every 12 lines down the sky.
        dc.w    $2c07,$fffe,COLOR01,$0f77
        dc.w    $3807,$fffe,COLOR01,$0fa7
        dc.w    $4407,$fffe,COLOR01,$0fd7
        dc.w    $5007,$fffe,COLOR01,$0ef7
        dc.w    $5c07,$fffe,COLOR01,$0bf7
        dc.w    $6807,$fffe,COLOR01,$08f7
        dc.w    $7407,$fffe,COLOR01,$07f9
        dc.w    $8007,$fffe,COLOR01,$07fc
        dc.w    $8c07,$fffe,COLOR01,$07ff
        dc.w    $9807,$fffe,COLOR01,$07cf
        dc.w    $a407,$fffe,COLOR01,$079f
        dc.w    $b007,$fffe,COLOR01,$087f
        dc.w    $bc07,$fffe,COLOR01,$0b7f
        dc.w    $c807,$fffe,COLOR01,$0e7f
        dc.w    $d407,$fffe,COLOR01,$0f7d
        dc.w    $e007,$fffe,COLOR01,$0f7a
        ; The boxes, from line 192 of the screen.
        dc.w    $ec07,$fffe
CopperDots:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    COLOR00,$0111,COLOR01,$0aaa
        dc.w    $ffff,$fffe

NoSprite:       dc.l    0

        section screens,bss_c

SkyA:           ds.b    SKYBYTES
SkyB:           ds.b    SKYBYTES
Dots:           ds.b    40*DOTLINES
