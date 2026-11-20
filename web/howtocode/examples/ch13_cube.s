; ch13_cube.s - a solid cube turning in three dimensions: a rotation matrix from three angles, perspective,
; back-face culling, faces drawn with the blitter's line mode and filled with its area fill, two screens (one
; on show while the other is drawn), and each face lit by how squarely it faces you. Left mouse button to leave.
;
; How To Code v8, chapter 13. CC0: use it for anything. The maths is Carl-Henrik Skaarstedt's (Asterix of
; Movement), from his chapter in How To Code v7; the code is new.
;
; Build and run it with F9. Set SHOW_TIME to 1 to see how much of the frame it takes: the background turns red
; while the processor works out the picture (the blitter clearing meanwhile), green while the edges are drawn,
; and blue while the fill finishes.
;
; $VER: ch13_cube 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        ifnd    SHOW_TIME
SHOW_TIME       equ     0
        endc

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
PLANESIZE       equ     ROWBYTES*HEIGHT
PLANES          equ     2               ; three face colours and the background
SCREENSIZE      equ     PLANESIZE*PLANES

CUBE            equ     64              ; half the cube's side
DISTANCE        equ     320             ; from the eye to the cube's centre (the screen is 256 from the eye)
CENTREX         equ     WIDTH/2
POINTS          equ     8
FACES           equ     6
BACKGROUND      equ     $012

; Each of the two screens has a few words of its own: where it is, and what was drawn in it last time, so
; only that much has to be cleared.
SCR_PLANES      equ     0               ; long: the first bitplane (the second follows it)
SCR_CLEARPTR    equ     4               ; long: the top left word of what was drawn
SCR_CLEARMOD    equ     8               ; word: its modulo
SCR_CLEARSIZE   equ     10              ; word: its BLTSIZE, or 0 if there's nothing to clear

; MULF: multiplies a word by a 2.14 fixed point number (a sine, or a matrix entry) and keeps the whole part.
; MULS gives 32 bits, 14 of them fraction; two doublings and a swap leave bits 14 to 29 in the low word: the
; same as shifting right by 14 (asr.l #8 then asr.l #6, 44 cycles), in 20.
; BLTCON1's bits for area mode (from the NDK's hardware/blit.i)
FILL_OR         equ     $08             ; inclusive fill
FILL_XOR        equ     $10             ; exclusive fill
FILL_CARRYIN    equ     $04             ; start each row filling, not empty
BLITREVERSE     equ     $02             ; descending

MULF    macro
        muls    \1,\2
        add.l   \2,\2
        add.l   \2,\2
        swap    \2
        endm

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; All eight sprites at an empty sprite, so the system's mouse pointer can't show.
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

        ; NTSC shows 200 lines: a shorter window, and the cube higher up.
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #$f4c1,CopperWindowEnd+2
        move.w  #100,CentreY
.pal:

        ; Both screens cleared, by the blitter: D only, minterm 0. Each is two bitplanes, 512 lines of 20 words.
        BLIT_BEGIN
        move.l  #$01000000,BLTCON0(a5)
        move.w  #0,BLTDMOD(a5)
        move.l  #Screen1,BLTDPTH(a5)
        move.w  #((HEIGHT*PLANES)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        jsr     WaitBlitter
        move.l  #Screen2,BLTDPTH(a5)
        move.w  #((HEIGHT*PLANES)<<6)!(ROWBYTES/2),BLTSIZE(a5)
        BLIT_END

        ; Screen 1 (empty) on show from the first frame; screen 2 is drawn first.
        move.l  #Screen1,PendingScreen
        move.w  Ramp1,PendingColours
        move.w  Ramp2,PendingColours+2
        move.w  Ramp3,PendingColours+4
        st      SwapPending
        bsr     VBlank                  ; puts them in the copper list, as it will every frame
        move.l  #ScreenInfo2,DrawScreen
        move.l  #ScreenInfo1,ShowScreen
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the next picture of the cube, into the screen not on show.

Frame:
        ; Don't draw into a screen until it has gone from the display. Taking the machine, Frame is only called
        ; after a vertical blank, so this never waits; with the system running, the startup's frame signal may
        ; have come while the last frame was still being drawn, and Frame would start again straight away.
.waitswap:
        tst.b   SwapPending
        beq.s   .swapped
        jsr     WaitFrame
        bra.s   .waitswap
.swapped:

        ifne    SHOW_TIME
        move.w  #$a00,COLOR00(a5)
        endc

        ; Turn: each angle a little more (they are byte offsets into the sine table, 1024 words to a turn).
        lea     Angles,a0
        add.w   #2*3,(a0)+
        add.w   #2*5,(a0)+
        add.w   #2*2,(a0)+

        BLIT_BEGIN
        move.l  DrawScreen,a4

        ; The blitter clears what was drawn in this screen two frames ago, one bitplane at a time, while the
        ; processor works out the new picture.
        moveq   #0,d1
        bsr     ClearPlane
        bsr     CalcMatrix
        bsr     RotatePoints
        move.l  #PLANESIZE,d1
        bsr     ClearPlane
        bsr     CullFaces
        bsr     FindBox

        ; Then the edges, and the fill (which has to wait for the last line). BLIT_END waits for the fill.
        ifne    SHOW_TIME
        move.w  #$0a0,COLOR00(a5)
        endc
        bsr     DrawEdges
        ifne    SHOW_TIME
        move.w  #$00c,COLOR00(a5)
        endc
        bsr     FillScreen
        BLIT_END

        bsr     LightFaces

        ; Ready: the vertical blank puts this screen on show, and the other one is drawn into next.
        move.l  SCR_PLANES(a4),PendingScreen
        st      SwapPending
        move.l  ShowScreen,DrawScreen
        move.l  a4,ShowScreen

        ifne    SHOW_TIME
        move.w  #BACKGROUND,COLOR00(a5)
        endc
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt: if a new picture is ready, its bitplanes and colours go into
; the copper list. The copper sets them at line $28, well after this has run and before the display starts, so
; it never sees half of a change.

VBlank:
        tst.b   SwapPending
        beq.s   .none
        lea     CopperPlanes,a0
        move.l  PendingScreen,d0
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)
        move.l  PendingScreen,d0
        add.l   #PLANESIZE,d0
        move.w  d0,14(a0)
        swap    d0
        move.w  d0,10(a0)
        lea     CopperColours,a0
        move.w  PendingColours,2(a0)
        move.w  PendingColours+2,6(a0)
        move.w  PendingColours+4,10(a0)
        sf      SwapPending
.none:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Starts the blitter clearing the box drawn in last time, in one bitplane: d1 = 0 for the first, PLANESIZE for
; the second. a4 = the screen's information.

ClearPlane:
        move.w  SCR_CLEARSIZE(a4),d0
        beq.s   .none
        move.l  SCR_CLEARPTR(a4),a0
        add.l   d1,a0
        jsr     WaitBlitter
        move.l  #$01000000,BLTCON0(a5)  ; D only, minterm 0: zeros
        move.w  SCR_CLEARMOD(a4),BLTDMOD(a5)
        move.l  a0,BLTDPTH(a5)
        move.w  d0,BLTSIZE(a5)
.none:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The rotation matrix from the three angles: Carl-Henrik's way, working out c1*s2 and s1*s2 once and using each
; twice. It turns about z (angle 1), then y (angle 2), then x (angle 3):
;
;   xx = c2*c1                  xy = -c2*s1                 xz = -s2
;   yx = s1*c3 - c1*s2*s3       yy = c1*c3 + s1*s2*s3       yz = -c2*s3
;   zx = s1*s3 + c1*s2*c3       zy = c1*s3 - s1*s2*c3       zz = c2*c3
;
; 14 multiplies, once a frame; then each point takes 9.

SC_C1   equ     0
SC_S1   equ     2
SC_C2   equ     4
SC_S2   equ     6
SC_C3   equ     8
SC_S3   equ     10

CalcMatrix:
        ; The sine and cosine of each angle. cos(a) = sin(a + a quarter turn): 256 words on, 512 bytes.
        lea     Sine,a0
        lea     Angles,a1
        lea     SinCos,a2
        moveq   #3-1,d7
.angle:
        move.w  (a1)+,d0
        and.w   #$7fe,d0                ; the angle wraps round: 1024 words
        move.w  (a0,d0.w),SC_S1(a2)
        add.w   #512,d0
        and.w   #$7fe,d0
        move.w  (a0,d0.w),SC_C1(a2)
        addq.l  #4,a2
        dbf     d7,.angle

        lea     SinCos,a0
        lea     Matrix,a1
        move.w  SC_C1(a0),d6            ; d6 = c1*s2
        MULF    SC_S2(a0),d6
        move.w  SC_S1(a0),d7            ; d7 = s1*s2
        MULF    SC_S2(a0),d7

        move.w  SC_C2(a0),d0            ; xx = c2*c1
        MULF    SC_C1(a0),d0
        move.w  d0,(a1)
        move.w  SC_C2(a0),d0            ; xy = -c2*s1
        MULF    SC_S1(a0),d0
        neg.w   d0
        move.w  d0,2(a1)
        move.w  SC_S2(a0),d0            ; xz = -s2
        neg.w   d0
        move.w  d0,4(a1)

        move.w  SC_S1(a0),d0            ; yx = s1*c3 - c1*s2*s3
        MULF    SC_C3(a0),d0
        move.w  d6,d1
        MULF    SC_S3(a0),d1
        sub.w   d1,d0
        move.w  d0,6(a1)
        move.w  SC_C1(a0),d0            ; yy = c1*c3 + s1*s2*s3
        MULF    SC_C3(a0),d0
        move.w  d7,d1
        MULF    SC_S3(a0),d1
        add.w   d1,d0
        move.w  d0,8(a1)
        move.w  SC_C2(a0),d0            ; yz = -c2*s3
        MULF    SC_S3(a0),d0
        neg.w   d0
        move.w  d0,10(a1)

        move.w  SC_S1(a0),d0            ; zx = s1*s3 + c1*s2*c3
        MULF    SC_S3(a0),d0
        move.w  d6,d1
        MULF    SC_C3(a0),d1
        add.w   d1,d0
        move.w  d0,12(a1)
        move.w  SC_C1(a0),d0            ; zy = c1*s3 - s1*s2*c3
        MULF    SC_S3(a0),d0
        move.w  d7,d1
        MULF    SC_C3(a0),d1
        sub.w   d1,d0
        move.w  d0,14(a1)
        move.w  SC_C2(a0),d0            ; zz = c2*c3
        MULF    SC_C3(a0),d0
        move.w  d0,16(a1)
        rts

; ---------------------------------------------------------------------------------------------------------------
; Each corner turned by the matrix and put in perspective: screen x = centre + x * 256 / (z + DISTANCE).
;
; The sum of three products is the new coordinate times 16384 (the matrix is 2.14). Times 256 is wanted, to
; divide by the distance, so it's shifted right by 6, not 14: no precision lost, and one shift instead of two.

RotatePoints:
        lea     Matrix,a1
        lea     CubePoints,a0
        lea     Projected,a2
        move.w  CentreY,a3
        moveq   #POINTS-1,d7
.point:
        movem.w (a0)+,d0-d2             ; x, y, z (each made a long, though only the words are used)

        move.w  d0,d5                   ; z first: it's needed to divide by
        muls    12(a1),d5
        move.w  d1,d3
        muls    14(a1),d3
        add.l   d3,d5
        move.w  d2,d3
        muls    16(a1),d3
        add.l   d3,d5
        add.l   d5,d5                   ; MULF's trick: the whole part of z, in the top word
        add.l   d5,d5
        swap    d5
        add.w   #DISTANCE,d5            ; how far it is from the eye: always more than 0 for this cube

        move.w  d0,d3                   ; x
        muls    (a1),d3
        move.w  d1,d4
        muls    2(a1),d4
        add.l   d4,d3
        move.w  d2,d4
        muls    4(a1),d4
        add.l   d4,d3
        asr.l   #6,d3                   ; x * 256
        divs    d5,d3                   ; / distance
        add.w   #CENTREX,d3
        move.w  d3,(a2)+

        move.w  d0,d3                   ; y
        muls    6(a1),d3
        move.w  d1,d4
        muls    8(a1),d4
        add.l   d4,d3
        move.w  d2,d4
        muls    10(a1),d4
        add.l   d4,d3
        asr.l   #6,d3
        divs    d5,d3
        add.w   a3,d3
        move.w  d3,(a2)+

        dbf     d7,.point
        rts

; ---------------------------------------------------------------------------------------------------------------
; Which faces are towards us. Each face's corners are listed clockwise as seen from outside the cube; on the
; screen they still go clockwise if the face is towards us, and anticlockwise if it's turned away. The sign of
; the 2D cross product of two of its edges tells which:
;
;   (x1-x0)*(y2-y0) - (y1-y0)*(x2-x0)   more than 0: clockwise (with y going down the screen): visible
;
; Each face visible gets its colour in FaceColour; each hidden one gets 0.

CullFaces:
        lea     Faces,a0
        lea     Projected,a1
        lea     FaceColour,a2
        moveq   #FACES-1,d7
.face:
        movem.w (a0)+,d0-d3             ; three corners (as offsets into Projected), and the colour
        movem.w (a1,d0.w),d4-d5         ; x0, y0
        movem.w (a1,d1.w),d0-d1         ; x1, y1
        movem.w (a1,d2.w),d2/d6         ; x2, y2
        sub.w   d4,d0                   ; x1-x0
        sub.w   d5,d1                   ; y1-y0
        sub.w   d4,d2                   ; x2-x0
        sub.w   d5,d6                   ; y2-y0
        muls    d6,d0
        muls    d2,d1
        sub.l   d1,d0
        bgt.s   .visible
        moveq   #0,d3
.visible:
        move.b  d3,(a2)+
        dbf     d7,.face
        rts

; ---------------------------------------------------------------------------------------------------------------
; The box round the eight corners on the screen: what the fill has to cover, and what is cleared from this
; screen next time it's drawn in. The box is kept in whole words, as the blitter works in words.

FindBox:
        lea     Projected,a0
        move.w  #WIDTH,d0               ; smallest x
        moveq   #0,d1                   ; largest x
        move.w  #HEIGHT,d2              ; smallest y
        moveq   #0,d3                   ; largest y
        moveq   #POINTS-1,d7
.point:
        movem.w (a0)+,d4-d5
        cmp.w   d0,d4
        bge.s   .1
        move.w  d4,d0
.1:     cmp.w   d1,d4
        ble.s   .2
        move.w  d4,d1
.2:     cmp.w   d2,d5
        bge.s   .3
        move.w  d5,d2
.3:     cmp.w   d3,d5
        ble.s   .4
        move.w  d5,d3
.4:     dbf     d7,.point

        lsr.w   #4,d0                   ; the first and last word across
        lsr.w   #4,d1
        move.w  d1,d4
        sub.w   d0,d4
        addq.w  #1,d4                   ; d4 = width in words
        move.w  d4,BoxWidth
        move.w  d3,d5
        sub.w   d2,d5
        addq.w  #1,d5                   ; d5 = height in lines
        move.w  d5,BoxHeight
        lsl.w   #6,d5
        or.w    d4,d5
        move.w  d5,SCR_CLEARSIZE(a4)    ; BLTSIZE: height << 6 + width
        add.w   d4,d4
        moveq   #ROWBYTES,d5
        sub.w   d4,d5
        move.w  d5,SCR_CLEARMOD(a4)     ; the modulo: what's left of each line

        move.l  SCR_PLANES(a4),a0       ; the top left word
        mulu    #ROWBYTES,d2
        add.l   d2,a0
        add.w   d0,d0
        add.w   d0,a0
        move.l  a0,SCR_CLEARPTR(a4)
        rts

; ---------------------------------------------------------------------------------------------------------------
; The edges. Each bitplane is filled on its own, so each needs the outline of the area it should be filled in:
; the faces whose colour has that bitplane's bit. An edge between two visible faces of colours 1 and 3 belongs
; in bitplane 1 (colour 3 has that bit, colour 1 hasn't), but not in bitplane 0, where both faces are filled
; and the edge has nothing to divide. So each edge goes in the bitplanes given by the exclusive or of its two
; faces' colours (0 for a hidden face): an edge between two hidden faces is in none, and isn't drawn at all.
; The lines are drawn by chapter 7's DrawFillLine, below.

DrawEdges:
        moveq   #ROWBYTES,d4            ; for DrawFillLine: from one line of a bitplane to the next
        move.l  SCR_PLANES(a4),a0       ; the first bitplane (DrawFillLine keeps a0, d4 and a2-a6)
        lea     Edges,a2
.edge:
        movem.w (a2)+,d0-d3             ; its two corners (offsets into Projected), and its two faces
        lea     FaceColour,a1
        move.b  (a1,d2.w),d5
        move.b  (a1,d3.w),d2
        eor.b   d2,d5                   ; the bitplanes it goes in
        beq.s   .next
        lea     Projected,a3
        lea     (a3,d1.w),a6            ; its second corner
        add.w   d0,a3                   ; its first
        move.w  d5,-(sp)
        btst    #0,d5
        beq.s   .plane2
        movem.w (a3),d0-d1
        movem.w (a6),d2-d3
        bsr     DrawFillLine
.plane2:
        btst    #1,1(sp)
        beq.s   .done
        add.l   #PLANESIZE,a0
        movem.w (a3),d0-d1
        movem.w (a6),d2-d3
        bsr     DrawFillLine
        sub.l   #PLANESIZE,a0
.done:
        addq.l  #2,sp
.next:
        cmp.l   #EdgesEnd,a2
        blo.s   .edge
        rts

; ---------------------------------------------------------------------------------------------------------------
; Fills the box in both bitplanes, one after the other, with chapter 7's FillArea: exclusive fill, so each
; shape's left edge is left out, and two faces side by side meet without a line of either colour between them.

FillScreen:
        move.l  SCR_CLEARPTR(a4),a0     ; the box's top left word, in the first bitplane
        bsr.s   .plane
        move.l  SCR_CLEARPTR(a4),a0
        add.l   #PLANESIZE,a0
.plane:
        move.w  BoxWidth,d0
        move.w  BoxHeight,d1
        move.w  SCR_CLEARMOD(a4),d2
        moveq   #FILL_XOR,d3
        bra     FillArea

; ---------------------------------------------------------------------------------------------------------------
; From here to FillArea: chapter 7's routines, from ch07_lines_fill.s, as they are there (without its DrawLine
; for solid lines, which this doesn't need). See chapter 7 for how they work.
;
; DrawFillLine draws from (d0,d1) to (d2,d3) into the bitplane at a0, which is d4 bytes from one line to the
; next (40 for a 320-pixel bitplane; more if the bitplanes are interleaved). a5 = CUSTOM. It doesn't clip: both
; ends must be on the bitplane. It changes d0-d3, d5-d7 and a1, and keeps d4, a0 and a2-a6.
;
; It waits for the blitter before setting it up, and starts the line without waiting for it to finish, so call
; it between BLIT_BEGIN and BLIT_END.
;
; DrawFillLine: a line for the area fill to fill between. It has one dot on each line of the screen it crosses
; (BLTCON1's SING bit), and it's drawn with exclusive-or (minterm $4A), so where two edges put a dot in the same
; place they cancel out. Its top end is left off - the line covers the screen lines below its top end down to its
; bottom end, never both ends - so where one edge ends and the next begins there's exactly one dot, not two. A
; level line covers no screen lines at all, and isn't drawn. All of which makes the fill come out right.

DrawFillLine:
        cmp.w   d1,d3
        beq.s   .level
        bgt.s   .down
        exg     d0,d2                   ; drawn from the top down, always
        exg     d1,d3
.down:
        move.w  #$0b4a,d5               ; A, C and D; LF $4A: D = ABc + aC, with B all ones D = A xor C
        moveq   #3,d6                   ; BLTCON1: LINE and SING
        bra.s   LineCommon
.level:
        rts

LineCommon:
        sub.w   d0,d2                   ; dx
        sub.w   d1,d3                   ; dy
        ; The octant: which way the line goes, as three bits of BLTCON1 (SUD, SUL and AUL). Index the table
        ; by: 1 if dx < 0, + 2 if dy < 0, + 4 if the line is more up and down than across. Then d2 is the
        ; longer of |dx| and |dy|, d3 the shorter.
        moveq   #0,d7
        tst.w   d2
        bpl.s   .dxplus
        neg.w   d2
        addq.w  #1,d7
.dxplus:
        tst.w   d3
        bpl.s   .dyplus
        neg.w   d3
        addq.w  #2,d7
.dyplus:
        cmp.w   d3,d2
        bhs.s   .across
        exg     d2,d3
        addq.w  #4,d7
.across:
        lea     Octants(pc),a1
        or.b    (a1,d7.w),d6

        ; The word with the first pixel in it, and that pixel's place in the word (BLTCON0's top four bits).
        mulu.w  d4,d1
        lea     (a0,d1.l),a1
        move.w  d0,d7
        lsr.w   #3,d7
        and.w   #$fffe,d7
        add.w   d7,a1
        and.w   #15,d0
        ror.w   #4,d0
        or.w    d0,d5

        ; The slope, as the HRM gives it: BLTBMOD = 4 * short, BLTAPTL = 4 * short - 2 * long (and SIGN set in
        ; BLTCON1 if that's negative), BLTAMOD = 4 * (short - long). The length is long + 1 pixels.
        add.w   d3,d3
        add.w   d3,d3                   ; 4 * short
        move.w  d3,d7
        sub.w   d2,d7
        sub.w   d2,d7                   ; 4 * short - 2 * long
        bpl.s   .plus
        or.w    #$40,d6                 ; SIGN
.plus:
        move.w  d7,d1
        sub.w   d2,d1
        sub.w   d2,d1                   ; 4 * (short - long)
        addq.w  #1,d2
        lsl.w   #6,d2
        addq.w  #2,d2                   ; BLTSIZE: the length in pixels as the height; the width is always 2

        jsr     WaitBlitter
        move.w  d5,BLTCON0(a5)          ; BLTCON0 before BLTADAT: the data is shifted as it's written
        move.w  d6,BLTCON1(a5)
        move.w  #$8000,BLTADAT(a5)      ; the one dot, moved along by the blitter
        move.w  #$ffff,BLTBDAT(a5)      ; the pattern: solid
        move.l  #$ffffffff,BLTAFWM(a5)  ; both masks all ones
        move.w  d4,BLTCMOD(a5)
        move.w  d4,BLTDMOD(a5)
        move.w  d3,BLTBMOD(a5)
        move.w  d1,BLTAMOD(a5)
        move.w  d7,BLTAPTL(a5)
        move.l  a1,BLTCPTH(a5)
        ; The blitter writes the line's first dot through BLTDPT and every dot after it through BLTCPT, so a
        ; fill line's first dot goes to a word nobody looks at: that's how its top end is left off.
        btst    #1,d6
        beq.s   .solid
        move.l  #LineDummy,BLTDPTH(a5)
        bra.s   .go
.solid:
        move.l  a1,BLTDPTH(a5)
.go:
        move.w  d2,BLTSIZE(a5)          ; last: this starts it
        rts

; BLTCON1's octant bits, by the index worked out above (the HRM's Table 6-3 octant in brackets).
Octants:
        dc.b    $10                     ; across, right, down (7)
        dc.b    $14                     ; across, left, down (4)
        dc.b    $18                     ; across, right, up (0)
        dc.b    $1c                     ; across, left, up (3)
        dc.b    $00                     ; up and down, right, down (6)
        dc.b    $08                     ; up and down, left, down (5)
        dc.b    $04                     ; up and down, right, up (1)
        dc.b    $0c                     ; up and down, left, up (2)

; ---------------------------------------------------------------------------------------------------------------
; FillArea: fills between the dots in a rectangle, with A to D copying it onto itself in descending mode.
;   a0 = the rectangle's top left word, d0 = its width in words (1 to 64), d1 = its height in rows (1 to 1024),
;   d2 = its modulo (the bytes from the end of one row to the start of the next), d3 = FILL_XOR or FILL_OR
;   (and FILL_CARRYIN if wanted), a5 = CUSTOM.
; Changes d1, d3 and d5-d7 and a1. Waits for the blitter before, not after: call between BLIT_BEGIN and BLIT_END.

FillArea:
        ; Descending mode goes backwards through memory: it starts at the rectangle's last word, bottom right.
        move.w  d0,d5
        add.w   d5,d5                   ; a row's bytes
        move.w  d5,d6
        add.w   d2,d6                   ; from one row to the next
        move.w  d1,d7
        subq.w  #1,d7
        mulu.w  d6,d7
        lea     -2(a0,d7.l),a1
        add.w   d5,a1                   ; the last word
        lsl.w   #6,d1
        move.w  d0,d5
        and.w   #63,d5                  ; 64 words wide is 0
        or.w    d5,d1
        or.w    #BLITREVERSE,d3

        jsr     WaitBlitter
        move.w  #$09f0,BLTCON0(a5)      ; A and D, LF $F0: D = A
        move.w  d3,BLTCON1(a5)
        move.l  #$ffffffff,BLTAFWM(a5)
        move.w  d2,BLTAMOD(a5)
        move.w  d2,BLTDMOD(a5)
        move.l  a1,BLTAPTH(a5)
        move.l  a1,BLTDPTH(a5)
        move.w  d1,BLTSIZE(a5)
        rts

; ---------------------------------------------------------------------------------------------------------------
; Light: each face as bright as it is square on to us. A face's normal (the direction it faces) is one of the
; matrix's columns: the x faces' is (xx,yx,zx), and so on. Its z part is how much it faces the viewer, so the
; colour for the front and back faces comes from zz, left and right from zx, top and bottom from zy. Only one of
; each pair is ever visible, so one colour serves both.

LightFaces:
        lea     Matrix,a0
        lea     PendingColours,a1
        move.w  16(a0),d0
        lea     Ramp1,a2
        bsr.s   .light
        move.w  12(a0),d0
        lea     Ramp2,a2
        bsr.s   .light
        move.w  14(a0),d0
        lea     Ramp3,a2
.light:
        bpl.s   .positive
        neg.w   d0
.positive:
        lsr.w   #8,d0                   ; 0 to 16384: 0 to 16 in steps of 1024...
        lsr.w   #2,d0
        cmp.w   #15,d0
        bls.s   .inrange
        moveq   #15,d0                  ; ...the top one held at 15
.inrange:
        add.w   d0,d0
        move.w  (a2,d0.w),(a1)+
        rts

        section data,data

Angles:         dc.w    0,0,0           ; as byte offsets into Sine
SinCos:         ds.w    6
Matrix:         ds.w    9               ; xx xy xz yx yy yz zx zy zz, 2.14 fixed point
Projected:      ds.w    POINTS*2        ; each corner's x and y on the screen
FaceColour:     ds.b    FACES           ; each face's colour, or 0 if it's turned away
        even
CentreY:        dc.w    HEIGHT/2
BoxWidth:       dc.w    0               ; the box round the cube this frame, in words...
BoxHeight:      dc.w    0               ; ...and lines
DrawScreen:     dc.l    0               ; the screen being drawn in
ShowScreen:     dc.l    0               ; the screen on show (or about to be)
PendingScreen:  dc.l    0               ; from Frame to VBlank: the screen to show next...
PendingColours: dc.w    0,0,0           ; ...and its three colours
SwapPending:    dc.b    0               ; ...and that they're ready
        even

ScreenInfo1:    dc.l    Screen1,0
                dc.w    0,0
ScreenInfo2:    dc.l    Screen2,0
                dc.w    0,0

; The cube's corners, and its faces: three of each face's corners (clockwise, from outside), as offsets into
; Projected, and its colour. Opposite faces share a colour: they're never both in view.
CubePoints:
        dc.w    -CUBE,-CUBE,-CUBE       ; 0
        dc.w    CUBE,-CUBE,-CUBE        ; 1
        dc.w    CUBE,CUBE,-CUBE         ; 2
        dc.w    -CUBE,CUBE,-CUBE        ; 3
        dc.w    -CUBE,-CUBE,CUBE        ; 4
        dc.w    CUBE,-CUBE,CUBE         ; 5
        dc.w    CUBE,CUBE,CUBE          ; 6
        dc.w    -CUBE,CUBE,CUBE         ; 7
Faces:
        dc.w    0*4,1*4,2*4,1           ; front (z = -CUBE, nearest at the start)
        dc.w    5*4,4*4,7*4,1           ; back
        dc.w    4*4,0*4,3*4,2           ; left
        dc.w    1*4,5*4,6*4,2           ; right
        dc.w    4*4,5*4,1*4,3           ; top
        dc.w    3*4,2*4,6*4,3           ; bottom
; The edges: two corners (offsets into Projected) and the two faces it's between.
Edges:
        dc.w    0*4,1*4,0,4
        dc.w    0*4,3*4,0,2
        dc.w    0*4,4*4,2,4
        dc.w    1*4,2*4,0,3
        dc.w    1*4,5*4,3,4
        dc.w    2*4,3*4,0,5
        dc.w    2*4,6*4,3,5
        dc.w    3*4,7*4,2,5
        dc.w    4*4,5*4,1,4
        dc.w    4*4,7*4,1,2
        dc.w    5*4,6*4,1,3
        dc.w    6*4,7*4,1,5
EdgesEnd:

; Each face colour from dim (turned almost edge on) to bright (square on): 16 steps.
Ramp1:  dc.w    $431,$531,$641,$741,$741,$852,$952,$962,$a62,$b62,$c72,$c72,$d83,$e83,$e93,$f93
Ramp2:  dc.w    $144,$145,$156,$156,$167,$267,$278,$289,$289,$29a,$29b,$2ab,$3ac,$3bd,$3bd,$3ce
Ramp3:  dc.w    $424,$425,$526,$527,$627,$638,$739,$839,$83a,$94b,$94c,$a4c,$a4d,$b5e,$b5e,$c5f

; A whole turn of sine in 1024 words: round(sin(2*pi*i/1024) * 16384), 2.14 fixed point. Made by a script;
; Limoniter's table generator (Tools, General, Table generator) makes the same: a sine, 1024 signed words,
; amplitude 16384.
Sine:
        dc.w    0,101,201,302,402,503,603,704,804,904,1005,1105,1205,1306,1406,1506
        dc.w    1606,1706,1806,1906,2006,2105,2205,2305,2404,2503,2603,2702,2801,2900,2999,3098
        dc.w    3196,3295,3393,3492,3590,3688,3786,3883,3981,4078,4176,4273,4370,4467,4563,4660
        dc.w    4756,4852,4948,5044,5139,5235,5330,5425,5520,5614,5708,5803,5897,5990,6084,6177
        dc.w    6270,6363,6455,6547,6639,6731,6823,6914,7005,7096,7186,7276,7366,7456,7545,7635
        dc.w    7723,7812,7900,7988,8076,8163,8250,8337,8423,8509,8595,8680,8765,8850,8935,9019
        dc.w    9102,9186,9269,9352,9434,9516,9598,9679,9760,9841,9921,10001,10080,10159,10238,10316
        dc.w    10394,10471,10549,10625,10702,10778,10853,10928,11003,11077,11151,11224,11297,11370,11442,11514
        dc.w    11585,11656,11727,11797,11866,11935,12004,12072,12140,12207,12274,12340,12406,12472,12537,12601
        dc.w    12665,12729,12792,12854,12916,12978,13039,13100,13160,13219,13279,13337,13395,13453,13510,13567
        dc.w    13623,13678,13733,13788,13842,13896,13949,14001,14053,14104,14155,14206,14256,14305,14354,14402
        dc.w    14449,14497,14543,14589,14635,14680,14724,14768,14811,14854,14896,14937,14978,15019,15059,15098
        dc.w    15137,15175,15213,15250,15286,15322,15357,15392,15426,15460,15493,15525,15557,15588,15619,15649
        dc.w    15679,15707,15736,15763,15791,15817,15843,15868,15893,15917,15941,15964,15986,16008,16029,16049
        dc.w    16069,16088,16107,16125,16143,16160,16176,16192,16207,16221,16235,16248,16261,16273,16284,16295
        dc.w    16305,16315,16324,16332,16340,16347,16353,16359,16364,16369,16373,16376,16379,16381,16383,16384
        dc.w    16384,16384,16383,16381,16379,16376,16373,16369,16364,16359,16353,16347,16340,16332,16324,16315
        dc.w    16305,16295,16284,16273,16261,16248,16235,16221,16207,16192,16176,16160,16143,16125,16107,16088
        dc.w    16069,16049,16029,16008,15986,15964,15941,15917,15893,15868,15843,15817,15791,15763,15736,15707
        dc.w    15679,15649,15619,15588,15557,15525,15493,15460,15426,15392,15357,15322,15286,15250,15213,15175
        dc.w    15137,15098,15059,15019,14978,14937,14896,14854,14811,14768,14724,14680,14635,14589,14543,14497
        dc.w    14449,14402,14354,14305,14256,14206,14155,14104,14053,14001,13949,13896,13842,13788,13733,13678
        dc.w    13623,13567,13510,13453,13395,13337,13279,13219,13160,13100,13039,12978,12916,12854,12792,12729
        dc.w    12665,12601,12537,12472,12406,12340,12274,12207,12140,12072,12004,11935,11866,11797,11727,11656
        dc.w    11585,11514,11442,11370,11297,11224,11151,11077,11003,10928,10853,10778,10702,10625,10549,10471
        dc.w    10394,10316,10238,10159,10080,10001,9921,9841,9760,9679,9598,9516,9434,9352,9269,9186
        dc.w    9102,9019,8935,8850,8765,8680,8595,8509,8423,8337,8250,8163,8076,7988,7900,7812
        dc.w    7723,7635,7545,7456,7366,7276,7186,7096,7005,6914,6823,6731,6639,6547,6455,6363
        dc.w    6270,6177,6084,5990,5897,5803,5708,5614,5520,5425,5330,5235,5139,5044,4948,4852
        dc.w    4756,4660,4563,4467,4370,4273,4176,4078,3981,3883,3786,3688,3590,3492,3393,3295
        dc.w    3196,3098,2999,2900,2801,2702,2603,2503,2404,2305,2205,2105,2006,1906,1806,1706
        dc.w    1606,1506,1406,1306,1205,1105,1005,904,804,704,603,503,402,302,201,101
        dc.w    0,-101,-201,-302,-402,-503,-603,-704,-804,-904,-1005,-1105,-1205,-1306,-1406,-1506
        dc.w    -1606,-1706,-1806,-1906,-2006,-2105,-2205,-2305,-2404,-2503,-2603,-2702,-2801,-2900,-2999,-3098
        dc.w    -3196,-3295,-3393,-3492,-3590,-3688,-3786,-3883,-3981,-4078,-4176,-4273,-4370,-4467,-4563,-4660
        dc.w    -4756,-4852,-4948,-5044,-5139,-5235,-5330,-5425,-5520,-5614,-5708,-5803,-5897,-5990,-6084,-6177
        dc.w    -6270,-6363,-6455,-6547,-6639,-6731,-6823,-6914,-7005,-7096,-7186,-7276,-7366,-7456,-7545,-7635
        dc.w    -7723,-7812,-7900,-7988,-8076,-8163,-8250,-8337,-8423,-8509,-8595,-8680,-8765,-8850,-8935,-9019
        dc.w    -9102,-9186,-9269,-9352,-9434,-9516,-9598,-9679,-9760,-9841,-9921,-10001,-10080,-10159,-10238,-10316
        dc.w    -10394,-10471,-10549,-10625,-10702,-10778,-10853,-10928,-11003,-11077,-11151,-11224,-11297,-11370,-11442,-11514
        dc.w    -11585,-11656,-11727,-11797,-11866,-11935,-12004,-12072,-12140,-12207,-12274,-12340,-12406,-12472,-12537,-12601
        dc.w    -12665,-12729,-12792,-12854,-12916,-12978,-13039,-13100,-13160,-13219,-13279,-13337,-13395,-13453,-13510,-13567
        dc.w    -13623,-13678,-13733,-13788,-13842,-13896,-13949,-14001,-14053,-14104,-14155,-14206,-14256,-14305,-14354,-14402
        dc.w    -14449,-14497,-14543,-14589,-14635,-14680,-14724,-14768,-14811,-14854,-14896,-14937,-14978,-15019,-15059,-15098
        dc.w    -15137,-15175,-15213,-15250,-15286,-15322,-15357,-15392,-15426,-15460,-15493,-15525,-15557,-15588,-15619,-15649
        dc.w    -15679,-15707,-15736,-15763,-15791,-15817,-15843,-15868,-15893,-15917,-15941,-15964,-15986,-16008,-16029,-16049
        dc.w    -16069,-16088,-16107,-16125,-16143,-16160,-16176,-16192,-16207,-16221,-16235,-16248,-16261,-16273,-16284,-16295
        dc.w    -16305,-16315,-16324,-16332,-16340,-16347,-16353,-16359,-16364,-16369,-16373,-16376,-16379,-16381,-16383,-16384
        dc.w    -16384,-16384,-16383,-16381,-16379,-16376,-16373,-16369,-16364,-16359,-16353,-16347,-16340,-16332,-16324,-16315
        dc.w    -16305,-16295,-16284,-16273,-16261,-16248,-16235,-16221,-16207,-16192,-16176,-16160,-16143,-16125,-16107,-16088
        dc.w    -16069,-16049,-16029,-16008,-15986,-15964,-15941,-15917,-15893,-15868,-15843,-15817,-15791,-15763,-15736,-15707
        dc.w    -15679,-15649,-15619,-15588,-15557,-15525,-15493,-15460,-15426,-15392,-15357,-15322,-15286,-15250,-15213,-15175
        dc.w    -15137,-15098,-15059,-15019,-14978,-14937,-14896,-14854,-14811,-14768,-14724,-14680,-14635,-14589,-14543,-14497
        dc.w    -14449,-14402,-14354,-14305,-14256,-14206,-14155,-14104,-14053,-14001,-13949,-13896,-13842,-13788,-13733,-13678
        dc.w    -13623,-13567,-13510,-13453,-13395,-13337,-13279,-13219,-13160,-13100,-13039,-12978,-12916,-12854,-12792,-12729
        dc.w    -12665,-12601,-12537,-12472,-12406,-12340,-12274,-12207,-12140,-12072,-12004,-11935,-11866,-11797,-11727,-11656
        dc.w    -11585,-11514,-11442,-11370,-11297,-11224,-11151,-11077,-11003,-10928,-10853,-10778,-10702,-10625,-10549,-10471
        dc.w    -10394,-10316,-10238,-10159,-10080,-10001,-9921,-9841,-9760,-9679,-9598,-9516,-9434,-9352,-9269,-9186
        dc.w    -9102,-9019,-8935,-8850,-8765,-8680,-8595,-8509,-8423,-8337,-8250,-8163,-8076,-7988,-7900,-7812
        dc.w    -7723,-7635,-7545,-7456,-7366,-7276,-7186,-7096,-7005,-6914,-6823,-6731,-6639,-6547,-6455,-6363
        dc.w    -6270,-6177,-6084,-5990,-5897,-5803,-5708,-5614,-5520,-5425,-5330,-5235,-5139,-5044,-4948,-4852
        dc.w    -4756,-4660,-4563,-4467,-4370,-4273,-4176,-4078,-3981,-3883,-3786,-3688,-3590,-3492,-3393,-3295
        dc.w    -3196,-3098,-2999,-2900,-2801,-2702,-2603,-2503,-2404,-2305,-2205,-2105,-2006,-1906,-1806,-1706
        dc.w    -1606,-1506,-1406,-1306,-1205,-1105,-1005,-904,-804,-704,-603,-503,-402,-302,-201,-101

; ---------------------------------------------------------------------------------------------------------------
; The copper list and the empty sprite, in chip RAM.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have (they ignore these)
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81
CopperWindowEnd:
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
        dc.w    BPLCON0,$2200           ; two bitplanes, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,BACKGROUND
CopperSprites:
        dc.w    SPR0PTH+$00,0,SPR0PTH+$02,0
        dc.w    SPR0PTH+$04,0,SPR0PTH+$06,0
        dc.w    SPR0PTH+$08,0,SPR0PTH+$0a,0
        dc.w    SPR0PTH+$0c,0,SPR0PTH+$0e,0
        dc.w    SPR0PTH+$10,0,SPR0PTH+$12,0
        dc.w    SPR0PTH+$14,0,SPR0PTH+$16,0
        dc.w    SPR0PTH+$18,0,SPR0PTH+$1a,0
        dc.w    SPR0PTH+$1c,0,SPR0PTH+$1e,0
        ; The screen and its colours are set at line $28, after VBlank has changed them and before the
        ; display starts at line $2c.
        dc.w    $2801,$fffe
CopperPlanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
CopperColours:
        dc.w    COLOR01,0,COLOR02,0,COLOR03,0
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

        section screens,bss_c

Screen1:
        ds.b    SCREENSIZE
Screen2:
        ds.b    SCREENSIZE
LineDummy:
        ds.w    1                       ; where each fill line's first dot goes, unseen (see DrawFillLine)
