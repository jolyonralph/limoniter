; ch17_multiplex.s - more moving objects than there are sprites: 24 balls on the eight sprite channels, each channel
; reused down the screen. Every frame the balls are sorted by height, and each is given the first channel that has
; finished its last ball at least a line above this one's top. The channels' colours differ (a colour for each
; pair of channels, as the hardware has it), so you can see the reuse. When more than eight balls share a line,
; the ninth gets no channel and isn't shown that frame. Left mouse button to leave.
;
; How To Code v8, chapter 17. CC0: use it for anything.
;
; $VER: ch17_multiplex 8.0 (21.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

OBJECTS         equ     24
CHANNELS        equ     8
BALLLINES       equ     12
LISTBYTES       equ     (OBJECTS*(BALLLINES+1)+1)*4 ; room for every ball on one channel, and the end
TOP             equ     $2c             ; the display's first line
XMAX            equ     320-16
YMIN            equ     60              ; the balls keep to a band, so they crowd onto the same lines
YMAX            equ     150

        section code,code

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
        move.w  #$f4c1,CopperWindowEnd2+2
.pal:
        ; The background: a line every 16 lines.
        lea     Screen,a0
        moveq   #0,d1
.row:
        moveq   #0,d0
        move.w  d1,d2
        and.w   #15,d2
        bne.s   .blank
        moveq   #-1,d0
.blank:
        moveq   #40/4-1,d2
.long:
        move.l  d0,(a0)+
        dbf     d2,.long
        addq.w  #1,d1
        cmp.w   #256,d1
        blo.s   .row
        move.l  #Screen,d0
        move.w  d0,CopperPlane+4
        move.w  d0,CopperPlane2+4
        swap    d0
        move.w  d0,CopperPlane
        move.w  d0,CopperPlane2

        ; The order the balls are drawn in, sorted again each frame: to start with, as they come.
        lea     Order,a0
        moveq   #0,d0
.order:
        move.b  d0,(a0)+
        addq.b  #1,d0
        cmp.b   #OBJECTS,d0
        blo.s   .order
        ; Spread the balls out, with different speeds: from a small table, turned about for each.
        lea     Balls,a0
        moveq   #0,d7
.ball:
        move.w  d7,d0
        mulu    #37,d0
        add.w   #8,d0
        and.w   #255,d0
        move.w  d0,(a0)+                ; x
        move.w  d7,d0
        mulu    #23,d0
        add.w   #5,d0
        and.w   #63,d0
        add.w   #YMIN+10,d0
        move.w  d0,(a0)+                ; y
        move.w  d7,d0
        and.w   #3,d0
        addq.w  #1,d0
        btst    #2,d7
        beq.s   .right
        neg.w   d0
.right:
        move.w  d0,(a0)+                ; x step
        move.w  d7,d0
        lsr.w   #1,d0
        and.w   #1,d0
        addq.w  #1,d0
        btst    #0,d7
        beq.s   .down
        neg.w   d0
.down:
        move.w  d0,(a0)+                ; y step
        addq.w  #1,d7
        cmp.w   #OBJECTS,d7
        blo.s   .ball
        ; Both copper lists' sprite pointers start at the empty lists in set B; the first Frame fills set A.
        lea     ListsB,a0
        moveq   #CHANNELS-1,d0
.empty:
        clr.l   (a0)
        lea     LISTBYTES(a0),a0
        dbf     d0,.empty
        lea     CopperSprites,a0
        lea     ListsB,a1
        bsr     PointSprites
        rts

; The eight sprite pointers in the copper list at a0, at the lists from a1 on.
PointSprites:
        moveq   #CHANNELS-1,d1
.pointer:
        move.l  a1,d0
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        move.w  d0,6(a0)
        addq.l  #8,a0
        lea     LISTBYTES(a1),a1
        dbf     d1,.pointer
        rts

; ---------------------------------------------------------------------------------------------------------------

Frame:
        bsr     MoveBalls
        bsr     SortBalls

        ; Every channel is free from the top. Free holds, for each channel, the first line a new ball on it may
        ; start on: one line after the last ball's end, as the channel needs that line to fetch the next ball's
        ; control words (the HRM: "at least one video line must separate the bottom of one usage of a sprite from
        ; the starting point of the next usage").
        lea     Free,a0
        moveq   #CHANNELS-1,d0
.free:
        clr.w   (a0)+
        dbf     d0,.free
        ; Where each channel's list has got to, in the set being made.
        move.l  BackLists,a1
        lea     Ends,a0
        moveq   #CHANNELS-1,d0
.ends:
        move.l  a1,(a0)+
        lea     LISTBYTES(a1),a1
        dbf     d0,.ends

        lea     Order,a3
        moveq   #OBJECTS-1,d7
.place:
        moveq   #0,d0
        move.b  (a3)+,d0
        lsl.w   #3,d0
        lea     Balls,a4
        add.w   d0,a4
        move.w  2(a4),d1
        add.w   #TOP,d1                 ; VSTART
        ; The first channel free by this line.
        lea     Free,a0
        moveq   #0,d2
.channel:
        cmp.w   (a0),d1
        bhs.s   .found
        addq.l  #2,a0
        addq.w  #1,d2
        cmp.w   #CHANNELS,d2
        blo.s   .channel
        bra.s   .next                   ; none: this ball isn't shown this frame
.found:
        move.w  d1,d3
        add.w   #BALLLINES,d3           ; VSTOP: the line after its last
        move.w  d3,d4
        addq.w  #1,d4
        move.w  d4,(a0)                 ; the channel is free again a line after that
        lea     Ends,a0
        add.w   d2,d2
        add.w   d2,d2
        add.w   d2,a0
        move.l  (a0),a1
        ; The control words: VSTART, VSTOP and HSTART, with their top bits in SPRxCTL.
        move.w  (a4),d0
        add.w   #$80,d0                 ; HSTART
        move.w  d1,d4
        lsl.w   #8,d4
        move.w  d0,d5
        lsr.w   #1,d5
        move.b  d5,d4
        move.w  d4,(a1)+
        move.w  d3,d4
        lsl.w   #8,d4
        btst    #8,d1
        beq.s   .v8
        bset    #2,d4
.v8:
        btst    #8,d3
        beq.s   .e8
        bset    #1,d4
.e8:
        btst    #0,d0
        beq.s   .h0
        bset    #0,d4
.h0:
        move.w  d4,(a1)+
        lea     Ball(pc),a2
        moveq   #BALLLINES-1,d0
.line:
        move.l  (a2)+,(a1)+
        dbf     d0,.line
        move.l  a1,(a0)
.next:
        dbf     d7,.place

        ; Each list ends with two words of 0.
        lea     Ends,a0
        moveq   #CHANNELS-1,d0
.end:
        move.l  (a0)+,a1
        clr.l   (a1)
        dbf     d0,.end

        ; This set into the other copper list, and that list on show from the next frame.
        move.l  BackCopper,a0
        add.l   #CopperSprites-Copper,a0
        move.l  BackLists,a1
        bsr     PointSprites
        move.l  BackCopper,COP1LC(a5)
        move.l  BackCopper,d0
        move.l  ShownCopper,BackCopper
        move.l  d0,ShownCopper
        move.l  BackLists,d0
        move.l  ShownLists,BackLists
        move.l  d0,ShownLists
        rts

MoveBalls:
        lea     Balls,a0
        moveq   #OBJECTS-1,d7
.ball:
        movem.w (a0),d0-d3
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
        cmp.w   #YMIN,d1
        blt.s   .yturn
        cmp.w   #YMAX,d1
        ble.s   .yok
.yturn:
        neg.w   d3
        add.w   d3,d1
        add.w   d3,d1
.yok:
        movem.w d0-d3,(a0)
        addq.l  #8,a0
        dbf     d7,.ball
        rts

; The balls in order of height, top first, by insertion sort. It's quick here: the balls move a line or two a
; frame, so last frame's order is nearly right, and insertion sort does little work on a list that nearly is.
SortBalls:
        lea     Order,a0
        lea     Balls+2,a1              ; each ball's y
        moveq   #1,d7
.next:
        move.b  (a0,d7.w),d0            ; the ball to put in place
        moveq   #0,d1
        move.b  d0,d1
        lsl.w   #3,d1
        move.w  (a1,d1.w),d2            ; its y
        move.w  d7,d6
.shift:
        subq.w  #1,d6
        bmi.s   .put
        moveq   #0,d1
        move.b  (a0,d6.w),d1
        lsl.w   #3,d1
        cmp.w   (a1,d1.w),d2
        bhs.s   .put
        move.b  (a0,d6.w),1(a0,d6.w)    ; that one moves down a place
        bra.s   .shift
.put:
        move.b  d0,1(a0,d6.w)
        addq.w  #1,d7
        cmp.w   #OBJECTS,d7
        blo.s   .next
        rts

VBlank:
        rts

; The ball: 12 lines of 16 pixels, colour 1 the edge, 2 the body, 3 the shine. Two words a line, as the sprite
; DMA reads them: the low bits of each pixel's colour, then the high bits.
Ball:
        dc.w    %0000011111100000,%0000000000000000
        dc.w    %0001100000011000,%0000011111100000
        dc.w    %0010001100000100,%0001111111111000
        dc.w    %0100011110000010,%0011111111111100
        dc.w    %0100011110000010,%0011111111111100
        dc.w    %1000001100000001,%0111111111111110
        dc.w    %1000000000000001,%0111111111111110
        dc.w    %0100000000000010,%0011111111111100
        dc.w    %0100000000000010,%0011111111111100
        dc.w    %0010000000000100,%0001111111111000
        dc.w    %0001100000011000,%0000011111100000
        dc.w    %0000011111100000,%0000000000000000

        section data,data

BackCopper:     dc.l    Copper2         ; the list made this frame, and the one on show
ShownCopper:    dc.l    Copper
BackLists:      dc.l    ListsA
ShownLists:     dc.l    ListsB
Free:           ds.w    CHANNELS
Ends:           ds.l    CHANNELS
Balls:          ds.w    4*OBJECTS       ; each: x, y, x step, y step
Order:          ds.b    OBJECTS
        even

        section copper,data_c

; Two copper lists, the same but for their sprite pointers: one on show, one being changed.
Copper:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200           ; one bitplane: faint lines every 16, to measure by
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0024           ; sprites in front
        dc.w    BPL1MOD,$0000
        dc.w    COLOR00,$0013
        dc.w    COLOR01,$0125
        dc.w    BPL1PTH
CopperPlane:
        dc.w    0,BPL1PTL,0
        dc.w    COLOR17,$0420,COLOR18,$0c62,COLOR19,$0fec ; sprites 0 and 1: orange
        dc.w    COLOR21,$0024,COLOR22,$026c,COLOR23,$0cef ; 2 and 3: blue
        dc.w    COLOR25,$0240,COLOR26,$04c4,COLOR27,$0efc ; 4 and 5: green
        dc.w    COLOR29,$0414,COLOR30,$0c4c,COLOR31,$0fcf ; 6 and 7: purple
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
CopperEnd:

Copper2:
        dc.w    FMODE,$0000
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd2:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$1200
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0024
        dc.w    BPL1MOD,$0000
        dc.w    COLOR00,$0013
        dc.w    COLOR01,$0125
        dc.w    BPL1PTH
CopperPlane2:
        dc.w    0,BPL1PTL,0
        dc.w    COLOR17,$0420,COLOR18,$0c62,COLOR19,$0fec
        dc.w    COLOR21,$0024,COLOR22,$026c,COLOR23,$0cef
        dc.w    COLOR25,$0240,COLOR26,$04c4,COLOR27,$0efc
        dc.w    COLOR29,$0414,COLOR30,$0c4c,COLOR31,$0fcf
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

        section lists,bss_c

ListsA:         ds.b    LISTBYTES*CHANNELS ; the sprite lists: one set on show, one being made
ListsB:         ds.b    LISTBYTES*CHANNELS
Screen:         ds.b    40*256
