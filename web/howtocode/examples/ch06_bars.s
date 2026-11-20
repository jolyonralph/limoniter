; ch06_bars.s - six copper bars swinging up and down the screen on a sine table, over a dark gradient. The copper
; list is double-buffered: each frame is built in a list the copper isn't reading, and the copper is pointed at it
; for the next frame through COP2LC. Left mouse button to leave.
;
; How To Code v8, chapter 6. CC0: use it for anything.
;
; $VER: ch06_bars 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

TOPLINE         equ     $2c             ; the display's first line
LINES           equ     256             ; lines the copper sets a colour on (PAL; NTSC shows the first 200)
BARS            equ     6
BARLINES        equ     16              ; how tall a bar is
LISTSIZE        equ     LINES*8+4+8+4   ; a WAIT and a MOVE a line, WAIT $FFDF, black below, and the end

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; How far the bars swing: over 256 lines on PAL, 200 on NTSC.
        move.w  #256,d0
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #200,d0
.pal:
        move.w  d0,d1
        sub.w   #BARLINES,d1
        lsr.w   #1,d1
        move.w  d1,BarCentre            ; the middle of the swing
        subq.w  #4,d1
        move.w  d1,BarSwing             ; and how far either way

        ; Both copper lists: the same WAITs; the colours are filled in each frame.
        lea     ListA,a0
        bsr     MakeList
        lea     ListB,a0
        bsr     MakeList

        ; Each bar's colours: its colour, from dim at the edges to full in the middle.
        lea     BarHues,a0
        lea     BarColours,a1
        moveq   #BARS-1,d7
.bar:
        move.w  (a0)+,d0                ; the bar's colour at full brightness
        lea     BarShape,a2
        moveq   #BARLINES-1,d6
.line:
        move.w  (a2)+,d1                ; the brightness here, 0 to 15
        moveq   #0,d5                   ; the colour being made
        moveq   #8,d4                   ; red, then green, then blue
.component:
        move.w  d0,d2
        lsr.w   d4,d2
        and.w   #$f,d2
        mulu    d1,d2
        divu    #15,d2                  ; component * brightness / 15
        lsl.w   d4,d2
        or.w    d2,d5
        subq.w  #4,d4
        bpl.s   .component
        move.w  d5,(a1)+
        dbf     d6,.line
        dbf     d7,.bar

        ; The background: from black at the top to dark blue at the bottom, a step every 32 lines.
        lea     Background,a0
        moveq   #0,d0
.background:
        move.w  d0,d1
        lsr.w   #5,d1                   ; 0 to 7: blue
        move.w  d0,d2
        lsr.w   #6,d2                   ; 0 to 3: a little green
        lsl.w   #4,d2
        or.w    d2,d1
        move.w  d1,(a0)+
        addq.w  #1,d0
        cmp.w   #LINES,d0
        blo.s   .background

        ; The copper starts with ListA; Frame builds ListB first.
        move.l  #ListA,COP2LC(a5)
        move.l  #ListB,Hidden
        rts

; A copper list of LINES lines at a0: WAIT for each, then MOVE to COLOR00 (the colour left 0 for now). Lines 256
; and on are written by their low 8 bits, after a WAIT for the end of line 255.
MakeList:
        move.w  #TOPLINE,d0
.line:
        cmp.w   #$100,d0
        bne.s   .notwrap
        move.l  #$ffdffffe,(a0)+
.notwrap:
        move.w  d0,d1
        lsl.w   #8,d1
        or.w    #$07,d1
        move.w  d1,(a0)+
        move.w  #$fffe,(a0)+
        move.l  #COLOR00<<16,(a0)+
        addq.w  #1,d0
        cmp.w   #TOPLINE+LINES,d0
        blo.s   .line
        move.w  d0,d1
        lsl.w   #8,d1
        or.w    #$07,d1
        move.w  d1,(a0)+
        move.w  #$fffe,(a0)+
        move.l  #COLOR00<<16,(a0)+      ; black below
        move.l  #$fffffffe,(a0)+
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: draw the bars into a colour for each line, copy those colours into the list the copper isn't
; reading, and tell the copper to read that one from the next frame on.

Frame:
        ; Start with the background.
        lea     Background,a0
        lea     LineColours,a1
        move.w  #LINES/2-1,d0
.clear:
        move.l  (a0)+,(a1)+
        dbf     d0,.clear

        ; Each bar a little further round the sine wave than the one before, so they follow each other.
        addq.w  #3,Angle
        lea     Sine,a2
        lea     BarColours,a3
        moveq   #0,d7                   ; the bar
.bar:
        move.w  Angle,d0
        move.w  d7,d1
        lsl.w   #4,d1                   ; 16 steps (of 256) apart
        add.w   d1,d0
        and.w   #255,d0
        add.w   d0,d0
        move.w  (a2,d0.w),d0            ; sine, -16384 to 16384
        muls    BarSwing,d0
        add.l   d0,d0
        add.l   d0,d0
        swap    d0                      ; * 4 / 65536: the same as / 16384
        add.w   BarCentre,d0            ; the bar's top line
        lea     LineColours,a1
        add.w   d0,d0
        add.w   d0,a1
        moveq   #BARLINES-1,d1
.line:
        move.w  (a3)+,(a1)+
        dbf     d1,.line
        addq.w  #1,d7
        cmp.w   #BARS,d7
        blo.s   .bar

        ; Into the hidden list: the colour word of each line's MOVE.
        move.l  Hidden,a0
        addq.l  #6,a0                   ; past the first WAIT and the MOVE's register
        lea     LineColours,a1
        move.w  #TOPLINE,d0
.copy:
        cmp.w   #$100,d0
        bne.s   .notwrap
        addq.l  #4,a0                   ; past the WAIT $FFDF
.notwrap:
        move.w  (a1)+,(a0)
        addq.l  #8,a0
        addq.w  #1,d0
        cmp.w   #TOPLINE+LINES,d0
        blo.s   .copy

        ; The copper jumps through COP2LC near the start of every frame (the COPJMP2 in Copper, below). It has
        ; already jumped this frame, into the other list, so this takes effect at the next frame: the list being
        ; shown is never the one being written.
        move.l  Hidden,d0
        move.l  d0,COP2LC(a5)
        cmp.l   #ListA,d0
        beq.s   .nowb
        move.l  #ListA,Hidden
        rts
.nowb:
        move.l  #ListB,Hidden
        rts

VBlank:
        rts

        section data,data

Angle:          dc.w    0
BarCentre:      dc.w    0
BarSwing:       dc.w    0
Hidden:         dc.l    0               ; the list Frame builds next

; The bars' colours at full brightness, and a bar's brightness from top to bottom.
BarHues:        dc.w    $0f22,$0f92,$0ee2,$02e4,$02af,$0a4f
BarShape:       dc.w    2,4,6,8,10,12,14,15,15,14,12,10,8,6,4,2

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

        section tables,bss

BarColours:     ds.w    BARS*BARLINES
Background:     ds.w    LINES
LineColours:    ds.w    LINES

; ---------------------------------------------------------------------------------------------------------------
; The copper list: the display's settings, then a jump to whichever list COP2LC points at.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    BPLCON0,$0200           ; no bitplanes: only the background colour
        dc.w    COLOR00,$0000
        dc.w    COPJMP2,$0000           ; writing anything here makes the copper jump to COP2LC

        section lists,bss_c

ListA:          ds.b    LISTSIZE
ListB:          ds.b    LISTSIZE
