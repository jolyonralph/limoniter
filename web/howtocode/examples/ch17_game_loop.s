; ch17_game_loop.s - a fixed-step game loop, and what goes wrong without one. Three blocks run back and forth along
; three rails at the same speed, 100 pixels a second, while every fourth frame takes the program a frame and a half
; (the red band shows the processor busy). The top one (white) is where the block should be: worked out from the
; time. The middle one (green) moves in fixed steps of 1/50 second, as many as the time asks for: it stays with the
; white one. The bottom one (red) moves once for each frame the program draws: every late frame, it falls behind.
; Run it on NTSC too: the green one keeps time, and without the late frames the red one would run 20% fast. Left
; mouse button to leave.
;
; How To Code v8, chapter 17. CC0: use it for anything.
;
; $VER: ch17_game_loop 8.0 (21.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

TICKRATE        equ     50              ; the game's steps a second, on PAL and on NTSC
MAXTICKS        equ     4               ; the most steps one frame may catch up
STEP            equ     2               ; pixels a step: 100 a second
TRACK           equ     320-16          ; how far a block goes before it turns
RAIL0           equ     $50             ; the rails' lines on the screen (as the beam counts them)
RAIL1           equ     $80
RAIL2           equ     $b0
BUSYLINE        equ     $90             ; a late frame ends here, a frame and a half after it began

        section code,code

Init:
        move.w  #50,FrameRate
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #60,FrameRate
        move.w  #$f4c1,CopperWindowEnd+2 ; NTSC: 200 lines
.pal:
        move.l  Frames,LastFrames
        move.l  Frames,FirstFrame
        move.w  FrameRate,d0
        mulu    #152,d0
        divu    #5,d0
        move.w  d0,Period               ; frames in five trips there and back: 1520 on PAL, 1824 on NTSC
        move.w  #$0000,COLOR00(a5)      ; the background: the copper list leaves it alone, for the busy band

        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        ; The sprites: block 0 white (sprite 0), 1 green (sprite 2), 2 red (sprite 4). Each list has room for one
        ; 16-line block; the rest of the eight point at an empty sprite.
        lea     CopperSprites,a0
        lea     SpriteTable(pc),a1
        moveq   #8-1,d1
.sprite:
        move.l  (a1)+,d0
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        dbf     d1,.sprite
        lea     Block0,a0
        bsr     MakeBlock
        lea     Block1,a0
        bsr     MakeBlock
        lea     Block2,a0
        bsr     MakeBlock

        ; Three thin rails in the bitplane, under the blocks.
        lea     Screen,a0
        move.w  #(RAIL0-$2c)*40,d0
        bsr     Rail
        move.w  #(RAIL1-$2c)*40,d0
        bsr     Rail
        move.w  #(RAIL2-$2c)*40,d0
        bsr     Rail
        bra     Place

Rail:
        lea     (a0,d0.w),a1
        moveq   #10-1,d1
.long:
        move.l  #-1,(a1)+
        dbf     d1,.long
        rts

SpriteTable:
        dc.l    Block0,NoSprite,Block1,NoSprite,Block2,NoSprite,NoSprite,NoSprite

; A block: 16 lines of colour 3 (both words set), and the end.
MakeBlock:
        addq.l  #4,a0                   ; the control words are set each frame
        moveq   #16-1,d0
.line:
        move.l  #$ffffffff,(a0)+
        dbf     d0,.line
        clr.l   (a0)
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame.

Frame:
        ; How many steps are due: one for every 1/50 second since the last frame. Each frame that's passed adds
        ; 50 to a clock, and each 50 (PAL) or 60 (NTSC) in it is a step: on NTSC, five steps every six frames.
        move.l  Frames,d0
        move.l  d0,d1
        sub.l   LastFrames,d0
        move.l  d1,LastFrames
        cmp.l   #MAXTICKS,d0
        bls.s   .few
        moveq   #MAXTICKS,d0
.few:
        mulu    #TICKRATE,d0
        add.w   TickClock,d0
        moveq   #0,d7
.count:
        cmp.w   FrameRate,d0
        blo.s   .counted
        sub.w   FrameRate,d0
        addq.w  #1,d7
        bra.s   .count
.counted:
        move.w  d0,TickClock
        cmp.w   #MAXTICKS,d7
        bls.s   .steps
        moveq   #MAXTICKS,d7            ; very late: give up the rest, so the game slows instead of falling behind
        clr.w   TickClock               ; for ever
.steps:
        move.w  Ticks,d0                ; each step moves the green block STEP pixels: here, all at once
        add.w   d7,d0
        bsr     Wrap
        move.w  d0,Ticks

        ; The red block: once for each frame drawn, however long it took. The mistake.
        move.w  Drawn,d0
        addq.w  #1,d0
        bsr     Wrap
        move.w  d0,Drawn

        bsr     Place

        ; Every fourth frame, too much to do: wait for the next frame and on to the middle of it.
        addq.w  #1,Count
        move.w  Count,d0
        and.w   #3,d0
        bne.s   .quick
        move.w  #$0600,COLOR00(a5)      ; the background red while the processor is "busy"
        move.l  Frames,d0
.wait:
        cmp.l   Frames,d0
        beq.s   .wait
.beam:
        move.l  VPOSR(a5),d0
        and.l   #$1ff00,d0
        cmp.l   #BUSYLINE<<8,d0
        blo.s   .beam
        move.w  #$0000,COLOR00(a5)
.quick:
        rts

; The steps are counted round a trip there and back (TRACK*2 pixels), so the counts never overflow.
Wrap:
        cmp.w   #TRACK*2/STEP,d0
        blo.s   .in
        sub.w   #TRACK*2/STEP,d0
.in:
        rts

; The blocks' sprites, at the distances each has gone. The white one's is from the time: 100 pixels a second,
; from the frames counted since the start and how many there are a second.
Place:
        move.l  Frames,d0
        sub.l   FirstFrame,d0
        divu    Period,d0               ; the frames since the last time all three were back at the start...
        clr.w   d0
        swap    d0
        mulu    #STEP*TICKRATE,d0       ; ...at 100 pixels a second...
        divu    FrameRate,d0            ; ...over 50 or 60 frames
        lea     Block0,a0
        move.w  #RAIL0-16,d1
        bsr     PlaceBlock
        move.w  Ticks,d0
        mulu    #STEP,d0
        lea     Block1,a0
        move.w  #RAIL1-16,d1
        bsr     PlaceBlock
        move.w  Drawn,d0
        mulu    #STEP,d0
        lea     Block2,a0
        move.w  #RAIL2-16,d1
        bra     PlaceBlock

; The sprite at a0 at line d1, d0.w pixels along a track it runs back and forth on.
PlaceBlock:
        and.l   #$ffff,d0
        divu    #TRACK*2,d0
        swap    d0                      ; how far into this trip there and back
        cmp.w   #TRACK,d0
        blo.s   .there
        neg.w   d0
        add.w   #TRACK*2,d0             ; on the way back
.there:
        add.w   #$80,d0                 ; HSTART: the screen's left edge
        move.w  d1,d2
        lsl.w   #8,d2
        move.w  d0,d3
        lsr.w   #1,d3
        move.b  d3,d2
        move.w  d2,(a0)                 ; SPRxPOS
        add.w   #16,d1
        lsl.w   #8,d1
        and.w   #1,d0
        or.w    d0,d1
        move.w  d1,2(a0)                ; SPRxCTL (the lines are all below 256, so no high bits)
        rts

VBlank:
        rts

        section data,data

FirstFrame:     dc.l    0
LastFrames:     dc.l    0
FrameRate:      dc.w    50
Period:         dc.w    1520
TickClock:      dc.w    0
Ticks:          dc.w    0
Drawn:          dc.w    0
Count:          dc.w    0

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
        dc.w    BPLCON2,$0024           ; sprites in front
        dc.w    BPL1MOD,$0000
        dc.w    COLOR01,$0444           ; the rails
        dc.w    COLOR19,$0fff           ; sprite 0's colour 3: white
        dc.w    COLOR23,$04e4           ; sprite 2's: green
        dc.w    COLOR27,$0f44           ; sprite 4's: red
CopperBitplane:
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
        dc.w    DMACON,DMAF_SETCLR!DMAF_SPRITE
        dc.w    $ffff,$fffe

NoSprite:       dc.l    0
Block0:         ds.l    1+16+1
Block1:         ds.l    1+16+1
Block2:         ds.l    1+16+1

        section screen,bss_c

Screen:         ds.b    40*256
