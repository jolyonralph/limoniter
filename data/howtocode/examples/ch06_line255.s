; ch06_line255.s - three copper bars bouncing across line 255, where the copper's WAIT runs out of bits. The copper
; list is made afresh each frame, with a WAIT only where the colour changes, and the WAIT for the end of line 255
; goes in wherever the first line after it falls. Two lists take turns: one is built while the copper shows the
; other, and COP1LC is pointed at the new one for the next frame. Left mouse button to leave.
;
; PAL only shows it all: an NTSC display ends at line 262 (and its usual window at line 244), so there the bars
; dip below the bottom of the screen.
;
; How To Code v8, chapter 6. CC0: use it for anything.
;
; $VER: ch06_line255 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

TOPLINE         equ     $2c             ; the display's first line
LINES           equ     256             ; the lines the bars can be on: $2C to $12B
BARS            equ     3
BARLINES        equ     24              ; how tall a bar is
MARKER          equ     $100            ; line 256: a thin grey line is drawn here, to show where it is
LISTSIZE        equ     HEADERSIZE+LINES*8+4+4

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: both lists get the display's settings at their start.

Init:
        lea     Copper,a0
        bsr     CopyHeader
        lea     CopperB,a0
        bsr     CopyHeader
        move.l  #$fffffffe,(a0)         ; until Frame first builds it, list B is only its header
        lea     Copper,a0
        add.w   #HEADERSIZE,a0
        move.l  #$fffffffe,(a0)
        move.l  #CopperB,Hidden

        ; Each bar's colours: a ramp up to its colour and down again.
        lea     BarHues,a0
        lea     BarColours,a1
        moveq   #BARS-1,d7
.bar:
        move.w  (a0)+,d0
        moveq   #0,d6                   ; the line within the bar
.line:
        ; Brightness: 15 at the middle, falling off towards the edges.
        move.w  d6,d1
        sub.w   #BARLINES/2,d1
        bpl.s   .abs
        not.w   d1                      ; -1 - d1: the same distance from the middle, either side
.abs:
        move.w  #15,d2
        sub.w   d1,d2
        moveq   #0,d5
        moveq   #8,d4
.component:
        move.w  d0,d3
        lsr.w   d4,d3
        and.w   #$f,d3
        mulu    d2,d3
        divu    #15,d3
        lsl.w   d4,d3
        or.w    d3,d5
        subq.w  #4,d4
        bpl.s   .component
        move.w  d5,(a1)+
        addq.w  #1,d6
        cmp.w   #BARLINES,d6
        blo.s   .line
        dbf     d7,.bar
        rts

CopyHeader:
        lea     Header,a1
        moveq   #HEADERSIZE/4-1,d0
.copy:
        move.l  (a1)+,(a0)+
        dbf     d0,.copy
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: draw the bars into a colour for each line, then write the list.

Frame:
        lea     LineColours,a0
        move.w  #LINES-1,d0
.clear:
        clr.w   (a0)+
        dbf     d0,.clear
        move.w  #$0666,LineColours+(MARKER-TOPLINE)*2

        ; Each bar bounces: its height is the size of a sine, so it hits the bottom and comes back up.
        lea     Sine,a2
        lea     BarColours,a3
        lea     BarSpeeds,a4
        moveq   #BARS-1,d7
.bar:
        move.w  (a4)+,d0
        mulu    Frames+2,d0             ; the bar's speed times the frame number: where it is in its bounce
        and.w   #127,d0                 ; half a turn: the sine is positive all the way
        add.w   d0,d0
        move.w  (a2,d0.w),d0            ; 0 to 16384
        mulu    #150,d0
        lsl.l   #2,d0
        swap    d0                      ; 0 to 150 lines up from the bottom
        move.w  #LINES-BARLINES,d1
        sub.w   d0,d1                   ; the bar's top line
        lea     LineColours,a1
        add.w   d1,d1
        add.w   d1,a1
        moveq   #BARLINES-1,d1
.line:
        move.w  (a3)+,(a1)+
        dbf     d1,.line
        dbf     d7,.bar

        ; The list: a WAIT and a MOVE only where a line's colour differs from the line above's.
        move.l  Hidden,a0
        add.w   #HEADERSIZE,a0
        lea     LineColours,a1
        moveq   #-1,d2                  ; the colour so far: none yet, so the first line always gets one
        moveq   #0,d3                   ; not zero once the WAIT for the end of line 255 is in
        move.w  #TOPLINE,d0
.list:
        move.w  (a1)+,d1
        cmp.w   d2,d1
        beq.s   .same
        move.w  d1,d2
        ; The first WAIT for line 256 or later needs a WAIT for the end of line 255 before it. After that, the
        ; copper's line count has wrapped round to 0, and WAITs for the lines that follow work as usual.
        cmp.w   #$100,d0
        blo.s   .wait
        tst.b   d3
        bne.s   .wait
        move.l  #$ffdffffe,(a0)+
        st      d3
.wait:
        move.w  d0,d4
        lsl.w   #8,d4                   ; only the line's low 8 bits fit
        or.w    #$07,d4
        move.w  d4,(a0)+
        move.w  #$fffe,(a0)+
        move.w  #COLOR00,(a0)+
        move.w  d1,(a0)+
.same:
        addq.w  #1,d0
        cmp.w   #TOPLINE+LINES,d0
        blo.s   .list
        move.l  #$fffffffe,(a0)+        ; the end

        ; The copper starts each frame from COP1LC, so this list is shown from the next frame. The one shown now
        ; is left alone, and built next frame.
        move.l  Hidden,d0
        move.l  d0,COP1LC(a5)
        cmp.l   #Copper,d0
        beq.s   .nowb
        move.l  #Copper,Hidden
        rts
.nowb:
        move.l  #CopperB,Hidden
        rts

VBlank:
        rts

        section data,data

Hidden:         dc.l    0               ; the list Frame builds next
BarHues:        dc.w    $0f42,$04f6,$046f
BarSpeeds:      dc.w    1,2,3           ; steps of the sine (256 to a turn) a frame

; The display's settings, at the start of both lists: no bitplanes, only the background colour.
Header:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have
        dc.w    BPLCON3,$0c00
        dc.w    BPLCON0,$0200
        dc.w    COLOR00,$0000
HEADERSIZE      equ     *-Header

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
LineColours:    ds.w    LINES

; ---------------------------------------------------------------------------------------------------------------
; The two copper lists, made by Init and Frame.

        section copper,bss_c

Copper:         ds.b    LISTSIZE
CopperB:        ds.b    LISTSIZE
