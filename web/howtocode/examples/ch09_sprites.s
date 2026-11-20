; ch09_sprites.s - AGA only: four sprites 64 pixels wide (FMODE's sprite bits), their colours moved to colours
; 224-239 with BPLCON4 and set in 24 bits, bouncing in steps of a quarter of a pixel (SPRxCTL's two extra position
; bits). On a machine without AGA it shows nothing and ends at once. Left mouse button to leave.
;
; How To Code v8, chapter 9. CC0: use it for anything.
;
; $VER: ch09_sprites 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

BALLS           equ     4               ; sprites 0, 2, 4 and 6; the odd ones are empty
SIZE            equ     64              ; each ball is 64 by 64
SPRITEBYTES     equ     16+SIZE*16+16   ; control (2 x 8 bytes), 64 lines of 2 x 8 bytes, the end (2 x 8 bytes)
LEFT            equ     $81*4           ; the display window, in quarter pixels (35ns, a super-hires pixel)
RIGHT           equ     ($1c1-SIZE)*4
TOP             equ     $2c
BOTTOMPAL       equ     $12c-SIZE       ; the lowest a ball's top line goes: PAL...
BOTTOMNTSC      equ     $f4-SIZE        ; ...and NTSC

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; AGA or not? Without it the copper list ends after its first move (no bitplanes), and the demo ends.
        bsr     IsAGA
        bne.s   .aga
        move.l  #$fffffffe,CopperAGA
        st      ExitNow
        rts
.aga:
        ; As the demo ends, the copper is pointed at a list that puts the AGA registers this demo changes back
        ; as they were (see Restore).
        move.l  #Restore,ExitHook

        ; Sprite data on an 8-byte boundary: 64-pixel sprites are fetched 64 bits at a time.
        move.l  #SpriteSpace+7,d0
        and.w   #$fff8,d0
        move.l  d0,SpriteStart
        bsr     DrawBalls

        ; All eight sprite pointers: the balls for the even ones, the empty sprite for the odd.
        lea     CopperSprites,a0
        move.l  SpriteStart,d0          ; ball 1
        move.l  d0,d1
        add.l   #BALLS*SPRITEBYTES,d1   ; the empty sprite, after the balls
        moveq   #BALLS-1,d2
.sprite:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        swap    d1
        move.w  d1,10(a0)
        swap    d1
        move.w  d1,14(a0)
        add.l   #SPRITEBYTES,d0
        lea     16(a0),a0
        dbf     d2,.sprite

        move.l  #EmptyRow,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2 ; NTSC: 200 lines
        move.w  #BOTTOMNTSC*16,Bottom
.pal:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Is this an AGA machine? Returns d0 = 1 (and the Z flag clear) if so, 0 if not.
;
; Lisa, AGA's video chip, answers a read of DENISEID ($DFF07C) with $F8 in the low byte. ECS's Denise answers $FC.
; The first Denise has no such register: a read gives whatever happens to be on the bus, which could be $F8 by
; chance. So read it many times: a chip that's really there gives the same answer every time.

IsAGA:
        move.w  DENISEID(a5),d0
        cmp.b   #$f8,d0
        bne.s   .no
        moveq   #32-1,d1
.again:
        move.w  DENISEID(a5),d2
        cmp.b   d0,d2
        bne.s   .no
        dbf     d1,.again
        moveq   #1,d0
        rts
.no:
        moveq   #0,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; The balls' pictures, the same in all four sprites. A 64-pixel sprite's data is the 16-pixel kind made four
; times as wide: its two control words are each the first word of 64 bits (the other 48 are ignored), and each
; line is 64 bits of the first plane then 64 bits of the second. Colour 1 is the edge, 2 the body, 3 a shine.

DrawBalls:
        move.l  SpriteStart,a0
        lea     16(a0),a0               ; the first line, past the control words
        moveq   #0,d7                   ; y
.line:
        moveq   #0,d0                   ; the first plane's 64 bits, in d0 (left half) and d1
        moveq   #0,d1
        moveq   #0,d2                   ; the second plane's, in d2 and d3
        moveq   #0,d3
        moveq   #0,d6                   ; x
.pixel:
        ; The distance squared from the middle (31.5, 31.5), in half pixels: (2x - 63)^2 + (2y - 63)^2.
        move.w  d6,d4
        add.w   d4,d4
        sub.w   #63,d4
        muls    d4,d4
        move.w  d7,d5
        add.w   d5,d5
        sub.w   #63,d5
        muls    d5,d5
        add.l   d5,d4
        moveq   #0,d5                   ; the colour: 0 outside
        cmp.l   #64*64,d4               ; radius 32
        bhs.s   .plot
        moveq   #1,d5
        cmp.l   #54*54,d4               ; the body inside radius 27
        bhs.s   .plot
        ; The shine: within 9 pixels of (22, 20).
        move.w  d6,d4
        sub.w   #22,d4
        muls    d4,d4
        move.w  d7,d5
        sub.w   #20,d5
        muls    d5,d5
        add.l   d5,d4
        moveq   #2,d5
        cmp.l   #9*9,d4
        bhs.s   .plot
        moveq   #3,d5
.plot:
        ; Shift the 64-bit rows left a pixel and put this pixel's two bits in at the right.
        add.l   d1,d1
        addx.l  d0,d0
        add.l   d3,d3
        addx.l  d2,d2
        btst    #0,d5
        beq.s   .a0
        addq.l  #1,d1
.a0:
        btst    #1,d5
        beq.s   .b0
        addq.l  #1,d3
.b0:
        addq.w  #1,d6
        cmp.w   #SIZE,d6
        blo     .pixel
        move.l  d0,(a0)+                ; SPRxDATA: 64 bits
        move.l  d1,(a0)+
        move.l  d2,(a0)+                ; SPRxDATB: 64 bits
        move.l  d3,(a0)+
        addq.w  #1,d7
        cmp.w   #SIZE,d7
        blo     .line

        ; The same picture for the other balls (the end and the empty sprite are left as zeros: the bss is clear).
        move.l  SpriteStart,a0
        lea     SPRITEBYTES(a0),a1
        move.w  #(BALLS-1)*SPRITEBYTES/4-1,d0
.copy:
        move.l  (a0)+,(a1)+
        dbf     d0,.copy
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: each ball moves, and its position goes into its control words.

Frame:
        tst.b   ExitNow                 ; no AGA: nothing to do
        bne     .done
        ; The startup leaves sprite DMA off when it takes the machine; on now, as the copper list has set every
        ; sprite pointer. (With the system running it's on already, for the mouse pointer.)
        move.w  #DMAF_SETCLR!DMAF_SPRITE,DMACON(a5)

        lea     Balls,a0
        move.l  SpriteStart,a1
        moveq   #BALLS-1,d7
.ball:
        ; Across, in quarter pixels: bounce off the window's sides.
        move.w  (a0),d0
        add.w   4(a0),d0
        cmp.w   #LEFT,d0
        blt.s   .xturn
        cmp.w   #RIGHT,d0
        ble.s   .xok
.xturn:
        neg.w   4(a0)
        add.w   4(a0),d0
        add.w   4(a0),d0
.xok:
        move.w  d0,(a0)
        ; Down, in lines: falling, and bouncing off the bottom.
        move.w  6(a0),d1
        addq.w  #1,d1                   ; gravity, in sixteenths of a line a frame
        move.w  2(a0),d2
        add.w   d1,d2
        cmp.w   Bottom,d2
        blt.s   .yok
        neg.w   d1                      ; bounce, losing nothing
        move.w  Bottom,d2
.yok:
        move.w  d1,6(a0)
        move.w  d2,2(a0)
        lsr.w   #4,d2                   ; the top line

        ; The control words. x is in quarter pixels: H0 and H1 (the 35ns and 70ns bits) are SPRxCTL's bits 3
        ; and 4; H2 (a whole pixel) is bit 0, as on every Amiga; H3-H10 are SPRxPOS's low byte.
        move.w  d2,d3                   ; SPRxPOS: the start line's low 8 bits, then H10-H3
        lsl.w   #8,d3
        move.w  d0,d4
        lsr.w   #3,d4
        move.b  d4,d3
        move.w  d3,(a1)
        move.w  d2,d3
        add.w   #SIZE,d3                ; the stop line
        move.w  d3,d4
        lsl.w   #8,d4                   ; SPRxCTL: the stop line's low 8 bits...
        btst    #8,d2
        beq.s   .sv8
        bset    #2,d4                   ; ...the start's bit 8 (SV8)...
.sv8:
        btst    #8,d3
        beq.s   .ev8
        bset    #1,d4                   ; ...the stop's bit 8 (EV8)...
.ev8:
        btst    #2,d0
        beq.s   .h2
        bset    #0,d4                   ; ...H2...
.h2:
        btst    #1,d0
        beq.s   .h1
        bset    #4,d4                   ; ...H1...
.h1:
        btst    #0,d0
        beq.s   .h0
        bset    #3,d4                   ; ...and H0.
.h0:
        move.w  d4,8(a1)                ; the second 64 bits' first word
        addq.l  #8,a0
        lea     SPRITEBYTES(a1),a1
        dbf     d7,.ball
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; As the demo ends (ExitHook). The system's copper lists set BPLCON3 and FMODE, but (without SetPatch, at least)
; not BPLCON4: it would stay as this demo left it, and the mouse pointer would take its colours from 224 on.
; Writing it from here isn't enough, as this demo's copper list could write it again before the copper is
; stopped; so the copper is pointed at a short list that puts it back, and the startup then gives the copper
; the system's own list.

Restore:
        move.l  #CopperRestore,COP1LC(a5)
        move.w  d0,COPJMP1(a5)
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do here.

VBlank:
        rts

        section data,data

; Each ball: x (in quarter pixels), y (in sixteenths of a line), x speed, y speed.
Balls:
        dc.w    LEFT+40*4,TOP*16,3,0
        dc.w    LEFT+100*4,(TOP+30)*16,-5,8
        dc.w    LEFT+160*4,(TOP+60)*16,2,-12
        dc.w    LEFT+220*4,(TOP+10)*16,-1,4

Bottom:         dc.w    BOTTOMPAL*16    ; the lowest a ball goes, in sixteenths of a line

        section bss,bss

SpriteStart:    ds.l    1

; ---------------------------------------------------------------------------------------------------------------
; The copper lists and the sprites, in chip RAM.

        section copper,data_c

Copper:
        dc.w    BPLCON0,$0200           ; no bitplanes until the rest is ready (without AGA, all there is)
CopperAGA:
        dc.w    FMODE,$000c             ; sprites 64 pixels wide (SPR32 and SPAGEM); bitplanes as before
        dc.w    BPLCON3,$0c40           ; SPRES = 01: sprite pixels are low resolution, whatever the screen's
        dc.w    BPLCON4,$00e1           ; even sprites' colours from 224 (ESPRM = 14), odd ones' from 16 as ever
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        ; One bitplane, all zeros: one row, shown on every line (the modulo takes the pointer back to its start).
        ; Only the sprites are seen, but they need it: Denise (and Lisa) show sprites on a line only once the
        ; bitplanes' data has started there.
        dc.w    BPL1MOD,-40
CopperBitplane:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPLCON0,$1200
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        ; Colours 224-239, in bank 7 (BPLCON3 bits 13-15 = 7), high halves then low halves. Each pair of
        ; sprites has 4 colours: 224-227 for sprites 0 and 1, 228-231 for 2 and 3, and so on. 224, 228, ... are
        ; transparent.
        ;
        ;       red:   $701018 $d02838 $ffd0c8          blue: $182a6a $3878e8 $d0e0ff
        ;       green: $0a5a28 $30b868 $e0ffe0          gold: $784410 $e8b038 $fff8e0
        dc.w    BPLCON3,$ec40
        dc.w    COLOR01,$711,COLOR02,$d23,COLOR03,$fdc    ; red
        dc.w    COLOR05,$052,COLOR06,$3b6,COLOR07,$efe    ; green
        dc.w    COLOR09,$126,COLOR10,$37e,COLOR11,$def    ; blue
        dc.w    COLOR13,$741,COLOR14,$eb3,COLOR15,$ffe    ; gold
        dc.w    BPLCON3,$ee40           ; LOCT: the low halves
        dc.w    COLOR01,$008,COLOR02,$088,COLOR03,$f08
        dc.w    COLOR05,$aa8,COLOR06,$088,COLOR07,$0f0
        dc.w    COLOR09,$8aa,COLOR10,$888,COLOR11,$00f
        dc.w    COLOR13,$840,COLOR14,$808,COLOR15,$f80
        dc.w    BPLCON3,$0c40           ; bank 0, LOCT clear
        dc.w    COLOR00,$0013           ; the background
        dc.w    $ffff,$fffe

; As the demo ends: the AGA registers back to what the system starts with.
CopperRestore:
        dc.w    BPLCON4,$0011
        dc.w    BPLCON3,$0c00
        dc.w    FMODE,$0000
        dc.w    $ffff,$fffe

EmptyRow:
        dcb.b   40,0

        section sprites,bss_c

SpriteSpace:
        ds.b    BALLS*SPRITEBYTES+16+8  ; the balls, an empty sprite (64 bits of control words, then the end),
                                        ; and 8 bytes to start on an 8-byte boundary
