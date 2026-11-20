; ch07_bobs.s - cookie-cut bobs: ten shaded balls bouncing over a background, drawn by the blitter into a screen
; that isn't shown, with the background put back where they were. Left mouse button to leave.
;
; How To Code v8, chapter 7. CC0: use it for anything.
;
; Three bitplanes, interleaved: each line of the screen is a line of bitplane 0, then of 1, then of 2. Then one
; blit draws or restores a bob in all three bitplanes, not three. The background uses colours 1 to 3 (bitplane 2
; clear), the balls 4 to 7 (bitplane 2 set). Everything is made by the program as it starts.
;
; $VER: ch07_bobs 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
PLANES          equ     3
ROWBYTES        equ     WIDTH/8         ; one line of one bitplane
LINEBYTES       equ     ROWBYTES*PLANES ; one line of the screen: all three bitplanes' lines, one after another
SCREENBYTES     equ     LINEBYTES*HEIGHT

BOBS            equ     10
BOBWIDTH        equ     32              ; pixels: two words
BOBHEIGHT       equ     32
BOBWORDS        equ     BOBWIDTH/16
BLITWORDS       equ     BOBWORDS+1      ; one more, for the bits shifted out of the last word
BOBBYTES        equ     BOBWORDS*2*PLANES*BOBHEIGHT
BOBSIZE         equ     ((BOBHEIGHT*PLANES)<<6)!BLITWORDS ; BLTSIZE: every bitplane's line is a row of the blit
SCREENMOD       equ     ROWBYTES-BLITWORDS*2 ; the screen's modulo: from the end of a row to the next row
XMAX            equ     WIDTH-BLITWORDS*16 ; so the blit (three words) stays inside the line
YMAX            equ     HEIGHT-BOBHEIGHT

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

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
        move.w  #$f4c1,CopperWindowEnd+2 ; NTSC: 200 lines
.pal:

        bsr     MakeBackground
        bsr     MakeBall

        ; Both screens start as the background: one blit each (A to D, 768 rows of 20 words).
        BLIT_BEGIN
        move.l  #$09f00000,BLTCON0(a5)  ; A and D, LF $F0 (D = A); BLTCON1 0
        move.l  #$ffffffff,BLTAFWM(a5)
        move.l  #0,BLTAMOD(a5)          ; BLTAMOD and BLTDMOD, which come next to each other
        move.l  #Background,BLTAPTH(a5)
        move.l  #ScreenA,BLTDPTH(a5)
        move.w  #((HEIGHT*PLANES)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        jsr     WaitBlitter
        move.l  #Background,BLTAPTH(a5)
        move.l  #ScreenB,BLTDPTH(a5)
        move.w  #((HEIGHT*PLANES)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        BLIT_END

        move.l  #ScreenA,d0
        bsr     ShowScreen
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: in the screen that isn't shown, put back the background where its bobs were two frames ago, move
; the bobs, draw them; then show that screen.

Frame:
        BLIT_BEGIN

        ; Put back the background. These registers are the same for every bob, so they're set once: nothing
        ; else uses the blitter until BLIT_END (that's what owning it means).
        move.l  DrawList,a2
        move.w  (a2)+,d7                ; how many to put back
        beq.s   .drawn
        subq.w  #1,d7
        move.l  #$09f00000,BLTCON0(a5)  ; A to D, LF $F0
        move.l  #$ffffffff,BLTAFWM(a5)
        move.w  #SCREENMOD,BLTAMOD(a5)
        move.w  #SCREENMOD,BLTDMOD(a5)
        move.l  DrawScreen,d1
.restore:
        move.l  (a2)+,d0                ; where the bob was, from the start of the screen
        move.l  d0,d2
        add.l   #Background,d2
        add.l   d1,d0
        jsr     WaitBlitter             ; the blit before must finish before its registers are changed
        move.l  d2,BLTAPTH(a5)
        move.l  d0,BLTDPTH(a5)
        move.w  #BOBSIZE,BLTSIZE(a5)
        dbf     d7,.restore
.drawn:

        ; Then the bobs, cookie-cut: A is the mask, B the ball, C the screen as it is, D the screen.
        ; D = AB + aC ($CA): where the mask is set, the ball; elsewhere, what was there.
        ; The ball is two words wide, the blit three: the third word of each row reads on into the next row's
        ; first word, so the last-word mask clears it ($0000), and the modulos of -2 take A and B back a word
        ; for the next row. That saves a blank word at the end of every row of the ball and its mask.
        jsr     WaitBlitter
        move.l  #$ffff0000,BLTAFWM(a5)  ; BLTAFWM $FFFF, BLTALWM $0000
        move.w  #-2,BLTAMOD(a5)
        move.w  #-2,BLTBMOD(a5)
        move.w  #SCREENMOD,BLTCMOD(a5)
        move.w  #SCREENMOD,BLTDMOD(a5)
        lea     Bobs,a3
        move.l  DrawList,a2
        move.w  #BOBS,(a2)+
        moveq   #BOBS-1,d7
.bob:
        ; Move it, bouncing off the edges. The processor does this while the blitter draws the bob before.
        movem.w (a3),d0-d3              ; x, y, x step, y step
        add.w   d2,d0
        bmi.s   .xturn
        cmp.w   #XMAX,d0
        ble.s   .xok
.xturn:
        neg.w   d2
        add.w   d2,d0
        add.w   d2,d0
.xok:
        add.w   d3,d1
        bmi.s   .yturn
        cmp.w   #YMAX,d1
        ble.s   .yok
.yturn:
        neg.w   d3
        add.w   d3,d1
        add.w   d3,d1
.yok:
        movem.w d0-d3,(a3)
        addq.l  #8,a3

        ; Where it goes: the word its left edge is in, and how far into that word (the shift, for A and B).
        mulu.w  #LINEBYTES,d1
        move.w  d0,d2
        lsr.w   #4,d2
        add.w   d2,d2
        add.w   d2,d1                   ; from the start of the screen
        move.l  d1,(a2)+                ; noted, to put the background back when this screen comes round again
        add.l   DrawScreen,d1
        and.w   #15,d0
        ror.w   #4,d0                   ; the shift, in the top four bits
        move.w  d0,d2
        or.w    #$0fca,d2               ; BLTCON0: A, B, C and D; LF $CA

        jsr     WaitBlitter
        move.w  d2,BLTCON0(a5)
        move.w  d0,BLTCON1(a5)          ; B's shift, the same
        move.l  #BallMask,BLTAPTH(a5)
        move.l  #Ball,BLTBPTH(a5)
        move.l  d1,BLTCPTH(a5)
        move.l  d1,BLTDPTH(a5)
        move.w  #BOBSIZE,BLTSIZE(a5)
        dbf     d7,.bob
        BLIT_END                        ; waits for the last bob to be drawn

        ; Show it from the next frame; the next frame draws in the other screen.
        move.l  DrawScreen,d0
        move.l  ShownScreen,DrawScreen
        move.l  d0,ShownScreen
        move.l  DrawList,d1
        move.l  ShownList,DrawList
        move.l  d1,ShownList
        bsr     ShowScreen
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do.

VBlank:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The screen at d0 into the copper list's bitplane pointers.

ShowScreen:
        lea     CopperBitplanes,a0
        moveq   #PLANES-1,d1
.plane:
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        add.l   #ROWBYTES,d0
        addq.l  #8,a0
        dbf     d1,.plane
        rts

; ---------------------------------------------------------------------------------------------------------------
; The background, by the processor: diagonal steps of 16 by 16 squares in colours 1, 2 and 3. Each word of each
; line is all one colour, so it's done a word at a time.

MakeBackground:
        lea     Background,a0
        moveq   #0,d7                   ; the line
.line:
        move.w  d7,d6
        lsr.w   #4,d6                   ; which row of squares
        moveq   #0,d5                   ; the bitplane
.plane:
        lea     Thirds(pc),a1
        add.w   d6,a1
        moveq   #ROWBYTES/2-1,d4
.word:
        moveq   #0,d0
        move.b  (a1)+,d0                ; the square's colour
        btst    d5,d0                   ; this bitplane's bit of it
        sne     d0
        ext.w   d0                      ; $FFFF or 0
        move.w  d0,(a0)+
        dbf     d4,.word
        addq.w  #1,d5
        cmp.w   #PLANES,d5
        blo.s   .plane
        addq.w  #1,d7
        cmp.w   #HEIGHT,d7
        blo.s   .line
        rts

Thirds:                                 ; enough for 16 rows of squares, 20 across
        dc.b    1,2,3,1,2,3,1,2,3,1,2,3,1,2,3,1,2,3
        dc.b    1,2,3,1,2,3,1,2,3,1,2,3,1,2,3,1,2,3
        even

; ---------------------------------------------------------------------------------------------------------------
; The ball, by the processor: a circle 32 pixels across, shaded in colours 4 to 7 from a highlight up and to the
; left, and its mask (set wherever the ball is). Interleaved like the screen: for each line of the ball, a line of
; bitplane 0, of 1, then of 2; and the mask has the same line three times, once for each bitplane.

MakeBall:
        lea     Ball,a0
        lea     BallMask,a1
        moveq   #0,d7                   ; the line
.line:
        moveq   #0,d0                   ; the mask's line, a pixel at a time from the left
        moveq   #0,d1                   ; bitplane 0's
        moveq   #0,d2                   ; bitplane 1's
        moveq   #0,d6                   ; the pixel
.pixel:
        add.l   d0,d0
        add.l   d1,d1
        add.l   d2,d2
        ; Distances in half pixels from the middle (so the circle is even): dx = 2x - 31, dy = 2y - 31.
        move.w  d6,d3
        add.w   d3,d3
        sub.w   #31,d3
        move.w  d7,d4
        add.w   d4,d4
        sub.w   #31,d4
        move.w  d3,d5
        muls.w  d5,d5
        move.w  d4,a2
        muls.w  d4,d4
        add.l   d4,d5                   ; dx^2 + dy^2
        cmp.l   #32*32,d5
        bhs.s   .outside
        addq.l  #1,d0                   ; inside the circle
        ; How near the highlight, at (-10, -10): the nearer, the lighter.
        add.w   #10,d3
        muls.w  d3,d3
        move.w  a2,d4
        add.w   #10,d4
        muls.w  d4,d4
        add.l   d4,d3
        cmp.l   #120,d3
        blo.s   .shade3
        cmp.l   #450,d3
        blo.s   .shade2
        cmp.l   #1000,d3
        blo.s   .shade1
        bra.s   .outside                ; shade 0: colour 4, bitplane 2 only
.shade3:
        addq.l  #1,d1
.shade2:
        addq.l  #1,d2
        bra.s   .outside
.shade1:
        addq.l  #1,d1
.outside:
        addq.w  #1,d6
        cmp.w   #BOBWIDTH,d6
        blo.s   .pixel

        move.l  d1,(a0)+                ; bitplane 0
        move.l  d2,(a0)+                ; bitplane 1
        move.l  d0,(a0)+                ; bitplane 2: set all over the ball, as its colours are 4 to 7
        move.l  d0,(a1)+                ; the mask, once for each bitplane
        move.l  d0,(a1)+
        move.l  d0,(a1)+
        addq.w  #1,d7
        cmp.w   #BOBHEIGHT,d7
        blo     .line
        rts

; ---------------------------------------------------------------------------------------------------------------

        section data,data

DrawScreen:     dc.l    ScreenB
ShownScreen:    dc.l    ScreenA
DrawList:       dc.l    ListB           ; where the bobs in each screen are, to put the background back
ShownList:      dc.l    ListA

; Each bob: x, y, and how far it moves each frame across and down.
Bobs:
        dc.w    10,20,2,1
        dc.w    60,150,-1,2
        dc.w    100,40,3,-2
        dc.w    150,100,-2,-1
        dc.w    200,10,1,3
        dc.w    240,180,-3,1
        dc.w    30,90,2,-3
        dc.w    180,60,-1,-2
        dc.w    260,130,2,2
        dc.w    120,200,-2,1

ListA:          dc.w    0
                ds.l    BOBS
ListB:          dc.w    0
                ds.l    BOBS

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the empty sprite, in chip RAM.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$3200           ; three bitplanes, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,LINEBYTES-ROWBYTES ; interleaved: skip the other two bitplanes' lines
        dc.w    BPL2MOD,LINEBYTES-ROWBYTES
        dc.w    COLOR00,$0000
        dc.w    COLOR01,$0225           ; the background
        dc.w    COLOR02,$0337
        dc.w    COLOR03,$0449
        dc.w    COLOR04,$0820           ; the ball, dark to light
        dc.w    COLOR05,$0c40
        dc.w    COLOR06,$0e80
        dc.w    COLOR07,$0ffc
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

; The ball and its mask: the blitter reads them, so they're in chip RAM.
        section ball,bss_c

Ball:           ds.b    BOBBYTES
BallMask:       ds.b    BOBBYTES
                ds.w    1               ; the blit reads one word past each (the third word of the last row)

        section screens,bss_c

Background:     ds.b    SCREENBYTES
ScreenA:        ds.b    SCREENBYTES
ScreenB:        ds.b    SCREENBYTES
