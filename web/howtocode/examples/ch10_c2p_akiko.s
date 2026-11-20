; ch10_c2p_akiko.s - AGA only, made for the CD32: the rotozoomer of ch10_c2p_cpu.s, its chunky pixels turned
; into 8 bitplanes by the CD32's Akiko chip, if it's there and can be used, or by the processor (C2P8) if not.
; The panel says which, and gives the times as ch10_c2p_cpu.s does, so the two can be compared on the same
; machine. On a machine without AGA it shows nothing and ends at once. Left mouse button to leave.
;
; How To Code v8, chapter 10. CC0: use it for anything.
;
; $VER: ch10_c2p_akiko 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320             ; the picture on the screen
HEIGHT          equ     176             ; 176 lines, then the panel: 200 in all, so it fits on NTSC too
ROWBYTES        equ     WIDTH/8         ; 40: a multiple of 8, as 4x fetch needs
PLANES          equ     8
ROWSTRIDE       equ     ROWBYTES*PLANES ; interleaved: each line has all eight planes' rows in turn
SCREENBYTES     equ     ROWSTRIDE*HEIGHT
CHUNKYW         equ     WIDTH           ; the chunky buffer: one byte a pixel
CHUNKYH         equ     HEIGHT
FIRSTLINE       equ     $2c
PANELLINE       equ     FIRSTLINE+HEIGHT+1      ; one blank line between the picture and the panel
PANELHEIGHT     equ     23
LASTLINE        equ     PANELLINE+PANELHEIGHT   ; $f4: the end of an NTSC display


; The texture: 256 texels across and 128 down, one byte each. Each row is stored twice over (512 bytes), and
; the whole 64K twice over (128K): then a texel's offset is u + v * 512 in a word, two offsets can be added
; without u carrying into v, and any word offset, read as a signed index from the middle, lands inside.
TEXBYTES        equ     512*128

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        tst.b   PalDisplay
        bne.s   .pal
        move.w  #263,LinesPerFrame
.pal:
        bsr     IsAGA
        bne.s   .aga
        move.l  #$fffffffe,CopperAGA    ; no AGA: the copper list ends after its first move
        st      ExitNow
        rts
.aga:
        ; Two screens on 8-byte boundaries, for 4x fetch: one shown while the other is drawn.
        move.l  #ScreenSpace+7,d0
        and.w   #$fff8,d0
        move.l  d0,ShowScreen
        add.l   #SCREENBYTES,d0
        move.l  d0,DrawScreen
        move.l  ShowScreen,PendingScreen
        st      SwapPending
        bsr     VBlank                  ; puts it in the copper list, as it will every frame

        move.l  #PanelSpace+7,d0
        and.w   #$fff8,d0
        move.l  d0,PanelStart
        lea     CopperPanel,a0
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)

        bsr     FindAkiko

        bsr     MakePalette
        bsr     MakeTexture
        bsr     WhichProcessor
        moveq   #0,d0
        moveq   #2,d1
        bsr     DrawText                ; a0 = the processor's name
        move.l  ModeText,a0             ; which conversion, set by FindAkiko
        moveq   #6,d0
        moveq   #2,d1
        bsr     DrawText
        lea     LabelsText,a0
        moveq   #0,d0
        moveq   #12,d1
        bsr     DrawText
        rts

; ---------------------------------------------------------------------------------------------------------------
; Is this an AGA machine? (Chapter 9.) Returns d0 = 1 (and the Z flag clear) if so, 0 if not.

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
; Once a frame (or rather, as often as it can): draw the next picture in chunky form, turn it into bitplanes in
; the screen not on show, and have VBlank show it. Each part is timed with the line counter.

Frame:
        tst.b   ExitNow                 ; no AGA: nothing to do
        bne     .done
        ; With the system running, a frame's signal can be waiting from while the last picture was being made:
        ; don't start on the screen still being shown before VBlank has swapped (see chapter 13).
.waitswap:
        tst.b   SwapPending
        beq.s   .swapped
        jsr     WaitFrame
        bra.s   .waitswap
.swapped:
        move.l  Frames,d0
        move.l  d0,d1
        sub.l   LastPicture,d1          ; frames since the last picture was shown
        move.l  d0,LastPicture
        move.w  d1,FramesPerPicture

        bsr     LineClock
        move.l  d0,TimeStart
        bsr     Render
        bsr     LineClock
        move.l  d0,d7
        sub.l   TimeStart,d0
        move.l  d0,RenderLines
        move.l  d7,TimeStart

        lea     Chunky,a0
        move.l  DrawScreen,a1
        move.l  AkikoC2PReg,d0
        beq.s   .cpu
        move.l  d0,a2
        bsr     AkikoC2P
        bra.s   .converted
.cpu:
        bsr     C2P8
.converted:
        lea     CUSTOM,a5

        bsr     LineClock
        sub.l   TimeStart,d0
        move.l  d0,C2PLines

        ; Show it at the next frame, and draw into the other screen next time.
        move.l  DrawScreen,d0
        move.l  ShowScreen,DrawScreen
        move.l  d0,ShowScreen
        move.l  d0,PendingScreen
        st      SwapPending

        bsr     ShowTimes
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame: put the screen Frame has finished into the copper list. The copper sets the
; bitplane pointers only after waiting for line $28, long after this has run, so it never sees half a change.

VBlank:
        tst.b   SwapPending
        beq.s   .none
        move.l  PendingScreen,d0
        lea     CopperBitplanes,a0
        moveq   #PLANES-1,d1
.plane:
        move.w  d0,6(a0)
        swap    d0
        move.w  d0,2(a0)
        swap    d0
        add.l   #ROWBYTES,d0
        addq.l  #8,a0
        dbf     d1,.plane
        sf      SwapPending
.none:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The time, in lines: the frames so far (Frames, counted by the vertical blank interrupt at line 0) times the
; lines in a frame, plus the line the beam is on now. Read again if the interrupt came in between, and wait out
; lines 0 and 1, so that the frame's interrupt has certainly been and gone. (CIA-B's time-of-day counter counts
; lines too, but with the system running graphics.library sets it back every frame.)
; Returns d0. Changes d1-d3.

LineClock:
        move.l  Frames,d1
        move.l  VPOSR+CUSTOM,d0
        cmp.l   Frames,d1
        bne.s   LineClock
        lsr.l   #8,d0
        and.l   #$1ff,d0                ; the line
        cmp.w   #2,d0
        blo.s   LineClock
        moveq   #0,d3
        move.w  LinesPerFrame,d3
        move.l  d1,d2                   ; Frames * lines a frame, in 32 bits
        swap    d2
        mulu    d3,d2
        swap    d2
        clr.w   d2
        mulu    d3,d1
        add.l   d2,d1
        add.l   d1,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; The rotozoomer. Every pixel of the picture is a texel of the texture, turned and zoomed: as x goes one pixel to
; the right, the texture position moves (du, dv); as y goes one line down, (-dv, du). Both in 8.8 fixed point.
;
; The step along a line is the same for every line, so the offsets of each pixel from the start of its line are
; worked out once, into Deltas (texel offsets: u + v * 512, so they add without u carrying into v); each line then
; has its own start, and each pixel is one table read and one texture read. (Adding the whole parts separately
; can be a texel out from the exact position, which doesn't show.)

Render:
        ; The angle turns once in 256 frames; the zoom swings between about 0.5 and 2 texels a pixel.
        move.l  Frames,d0
        lea     Sine,a0
        move.w  d0,d1
        and.w   #255,d1
        add.w   d1,d1
        move.w  0(a0,d1.w),d2           ; sin(angle), 2.14
        add.w   #64*2,d1                ; cos = sin a quarter turn on
        and.w   #511,d1
        move.w  0(a0,d1.w),d3           ; cos(angle)
        move.w  d0,d1
        mulu    #3,d1                   ; the zoom swings at 3/2 the speed the angle turns
        lsr.w   #1,d1
        and.w   #255,d1
        add.w   d1,d1
        move.w  0(a0,d1.w),d4
        muls    #192,d4
        asr.l   #8,d4
        asr.l   #6,d4                   ; sin * 192: -192 to 192
        add.w   #320,d4                 ; zoom: 128 to 512 (0.5 to 2 texels a pixel), 8.8
        muls    d4,d3
        asr.l   #8,d3
        asr.l   #6,d3                   ; du = cos * zoom, 8.8
        muls    d4,d2
        asr.l   #8,d2
        asr.l   #6,d2                   ; dv = sin * zoom
        move.w  d3,d6                   ; du
        move.w  d2,d7                   ; dv

        ; Deltas: for x from -CHUNKYW/2, the offset of (x * du, x * dv).
        lea     Deltas,a2
        move.w  #-CHUNKYW/2,d0
        muls    d6,d0                   ; u
        move.w  #-CHUNKYW/2,d1
        muls    d7,d1                   ; v
        ext.l   d6
        ext.l   d7
        move.w  #CHUNKYW-1,d5
.delta:
        bsr     TexelOffset
        move.w  d2,(a2)+
        add.l   d6,d0
        add.l   d7,d1
        dbf     d5,.delta

        ; The centre of the picture moves across the texture.
        move.l  Frames,d0
        move.l  d0,d1
        lsl.l   #8,d0                   ; u0 = frames * 1 texel and a bit
        add.l   d1,d0
        mulu    #180,d1                 ; v0 = frames * 0.7 texels

        ; The first line's start: y = -CHUNKYH/2, so (u, v) = (u0 - y * dv, v0 + y * du).
        move.w  #CHUNKYH/2,d2
        muls    d7,d2
        add.l   d2,d0                   ; u0 + CHUNKYH/2 * dv
        move.w  #CHUNKYH/2,d2
        muls    d6,d2
        sub.l   d2,d1                   ; v0 - CHUNKYH/2 * du

        lea     Chunky,a1
        move.w  #CHUNKYH-1,d5
.line:
        bsr     TexelOffset
        lea     Texture+32768,a0        ; the offsets are signed words: from 32K in, line start + offset is
        add.l   d2,a0                   ; always inside the 128K
        lea     Deltas,a2
        moveq   #CHUNKYW/32-1,d4
.pixels:
        rept    32
        move.w  (a2)+,d3
        move.b  0(a0,d3.w),(a1)+
        endr
        dbf     d4,.pixels
        sub.l   d7,d0                   ; down a line: u -= dv, v += du
        add.l   d6,d1
        dbf     d5,.line
        rts

; (u, v) in d0 and d1 (8.8) to a texel offset in d2: (u & 255) + (v & 127) * 512, zero-extended to a long.
TexelOffset:
        move.l  d1,d2
        lsr.l   #8,d2
        and.l   #127,d2
        lsl.w   #8,d2
        add.w   d2,d2                   ; v * 512
        move.l  d0,d3
        lsr.l   #8,d3
        and.w   #255,d3
        or.w    d3,d2
        rts

; ---------------------------------------------------------------------------------------------------------------
; Is there an Akiko to use? graphics.library V40 and later (Kickstart 3.1) keeps the address of the chunky to
; planar hardware in GfxBase, as gb_ChunkyToPlanarPtr, or 0 if there's none: read it there, rather than assume
; $B80038. On a 68030 or later, use the processor instead: they have a data cache, which could keep a copy of
; what was read from Akiko's register the first time and give it back every time after (c2p.library uses
; Akiko only with a 68020 for the same reason), and they're fast enough that Akiko gains little or nothing.
; Sets AkikoC2PReg and ModeText.

gb_ChunkyToPlanarPtr equ 508            ; in GfxBase, from V40 (the NDK 3.2's graphics/gfxbase.i)

FindAkiko:
        clr.l   AkikoC2PReg
        move.l  #NoAkikoText,ModeText
        move.l  GfxBase,a6
        cmp.w   #40,LIB_VERSION(a6)
        blo.s   .done                   ; older: no such field
        move.l  gb_ChunkyToPlanarPtr(a6),d0
        beq.s   .done
        move.l  ExecBase.w,a6
        btst    #AFB_68030,AttnFlags+1(a6)
        beq.s   .use
        move.l  #CachedText,ModeText
        bra.s   .done
.use:
        move.l  d0,AkikoC2PReg
        move.l  #AkikoText,ModeText
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Chunky to planar by Akiko: write 8 longwords (32 chunky pixels) to its register, read 8 back, one for each
; bitplane from bitplane 0 up, and put them in the screen. a0 = the chunky buffer, a1 = the screen, a2 = the
; register. Changes d6, d7, a0 and a1.
;
; Akiko is shared through the blitter: own the blitter while using it (the WriteChunkyPixels autodoc says so).
; Taking the machine, the startup owns it already; with the system running, BLIT_BEGIN and BLIT_END own it.

; ---- akikoc2p begin
AkikoC2P:
        BLIT_BEGIN
        move.w  #CHUNKYH-1,d7
.row:
        moveq   #CHUNKYW/32-1,d6
.block:
        move.l  (a0)+,(a2)              ; 32 pixels in...
        move.l  (a0)+,(a2)
        move.l  (a0)+,(a2)
        move.l  (a0)+,(a2)
        move.l  (a0)+,(a2)
        move.l  (a0)+,(a2)
        move.l  (a0)+,(a2)
        move.l  (a0)+,(a2)
        move.l  (a2),(a1)               ; ...and 32 pixels of each bitplane out
        move.l  (a2),1*ROWBYTES(a1)
        move.l  (a2),2*ROWBYTES(a1)
        move.l  (a2),3*ROWBYTES(a1)
        move.l  (a2),4*ROWBYTES(a1)
        move.l  (a2),5*ROWBYTES(a1)
        move.l  (a2),6*ROWBYTES(a1)
        move.l  (a2),7*ROWBYTES(a1)
        addq.l  #4,a1
        dbf     d6,.block
        add.w   #ROWSTRIDE-ROWBYTES,a1
        dbf     d7,.row
        BLIT_END
        rts
; ---- akikoc2p end

; ---------------------------------------------------------------------------------------------------------------
; Chunky to planar, 8 bitplanes, 1x1 pixels: CHUNKYW by CHUNKYH bytes at a0 to the interleaved screen at a1.
; Changes every register but a7.
;
; Each 32 pixels (8 longwords, L0 to L7, four pixels in each) become 8 longwords, one of each bitplane. Think of
; the 256 bits as a table to be transposed. A merge swaps a group of bits between two registers: with s = 16,
; the low word of one with the high word of the other; with s = 8, every other byte; and so on to single bits.
; Five rounds of merges, s = 16, 8, 4, 2 and 1, four merges in each, transpose the whole table. Which pairs of
; registers each round merges is worked out so the work splits in two:
;
;   s = 4  on (L0,L1) (L2,L3) (L4,L5) (L6,L7)
;   then the even ones, L0 L2 L4 L6, on their own: s = 16 and s = 2 on (L0,L4) (L2,L6); s = 8 and s = 1 on
;   (L0,L2) (L4,L6) - and the odd ones the same.
;
; That leaves bitplane 7 in L0, 3 in L1, 6 in L2, 2 in L3, 5 in L4, 1 in L5, 4 in L6, 0 in L7.
;
; The writes to chip RAM are spread through the work, not bunched at the end: a 68020 or 68030 can go on with
; the next instructions (from its cache) while a write it has started waits for its turn at chip RAM. So the
; odd half's four planes are written during the next block's first round, and the even half's during the odd
; half's merges. a6 is where the last block went; a2-a5 hold the planes waiting to be written.

; A merge: swap the bits of \1 picked by the mask \4 with the bits of \2 that are \3 places to their left.
MERGE           macro
        move.l  \2,d5
        lsr.l   #\3,d5
        eor.l   \1,d5
        and.l   #\4,d5
        eor.l   d5,\1
        lsl.l   #\3,d5
        eor.l   d5,\2
        endm

; The same with s = 16: \1's low word swapped with \2's high word, without a spare register.
MERGE16         macro
        swap    \2
        eor.w   \1,\2
        eor.w   \2,\1
        eor.w   \1,\2
        swap    \2
        endm

; ---- c2p8 begin
C2P8:
        lea     C2PScratch,a6           ; the "last block" for the first: its four writes land here
        move.w  #CHUNKYH-1,d7
.row:
        moveq   #CHUNKYW/32-1,d6
.block:
        ; s = 4 on each pair, writing the last block's odd planes, and keeping this block's odd longwords in
        ; a2-a5 in their place.
        move.l  (a0)+,d0
        move.l  (a0)+,d4
        MERGE   d0,d4,4,$0f0f0f0f
        move.l  a2,3*ROWBYTES(a6)       ; L1: bitplane 3
        move.l  d4,a2
        move.l  (a0)+,d1
        move.l  (a0)+,d4
        MERGE   d1,d4,4,$0f0f0f0f
        move.l  a3,2*ROWBYTES(a6)       ; L3: bitplane 2
        move.l  d4,a3
        move.l  (a0)+,d2
        move.l  (a0)+,d4
        MERGE   d2,d4,4,$0f0f0f0f
        move.l  a4,1*ROWBYTES(a6)       ; L5: bitplane 1
        move.l  d4,a4
        move.l  (a0)+,d3
        move.l  (a0)+,d4
        MERGE   d3,d4,4,$0f0f0f0f
        move.l  a5,0*ROWBYTES(a6)       ; L7: bitplane 0
        move.l  d4,a5

        ; The even longwords: d0-d3 = L0 L2 L4 L6.
        MERGE16 d0,d2
        MERGE16 d1,d3
        MERGE   d0,d2,2,$33333333
        MERGE   d1,d3,2,$33333333
        MERGE   d0,d1,8,$00ff00ff
        MERGE   d2,d3,8,$00ff00ff
        MERGE   d0,d1,1,$55555555
        MERGE   d2,d3,1,$55555555

        ; Swap the finished even planes for the odd longwords, and do those, writing the even planes meanwhile.
        exg     d0,a2
        exg     d1,a3
        exg     d2,a4
        exg     d3,a5
        MERGE16 d0,d2
        MERGE16 d1,d3
        move.l  a2,7*ROWBYTES(a1)       ; L0: bitplane 7
        MERGE   d0,d2,2,$33333333
        MERGE   d1,d3,2,$33333333
        move.l  a3,6*ROWBYTES(a1)       ; L2: bitplane 6
        MERGE   d0,d1,8,$00ff00ff
        MERGE   d2,d3,8,$00ff00ff
        move.l  a4,5*ROWBYTES(a1)       ; L4: bitplane 5
        MERGE   d0,d1,1,$55555555
        MERGE   d2,d3,1,$55555555
        move.l  a5,4*ROWBYTES(a1)       ; L6: bitplane 4

        ; The odd planes wait for the next block.
        move.l  d0,a2
        move.l  d1,a3
        move.l  d2,a4
        move.l  d3,a5
        move.l  a1,a6
        addq.l  #4,a1
        dbf     d6,.block
        add.w   #ROWSTRIDE-ROWBYTES,a1  ; on to the next line's first plane
        dbf     d7,.row

        ; The last block's odd planes.
        move.l  a2,3*ROWBYTES(a6)
        move.l  a3,2*ROWBYTES(a6)
        move.l  a4,1*ROWBYTES(a6)
        move.l  a5,0*ROWBYTES(a6)
        rts
; ---- c2p8 end



; ---------------------------------------------------------------------------------------------------------------
; The texture: u XOR (v * 2), 256 by 128 texels, every row twice over and the whole twice over (see TEXBYTES).

MakeTexture:
        lea     Texture,a0
        move.w  #256-1,d7               ; 256 rows: the 128 twice
.row:
        move.w  d7,d1
        not.w   d1
        add.w   d1,d1                   ; v * 2 (the top bit falls off as a byte)
        move.w  #512-1,d6
        moveq   #0,d0                   ; u
.texel:
        move.b  d0,d2
        eor.b   d1,d2
        move.b  d2,(a0)+
        addq.b  #1,d0
        dbf     d6,.texel
        dbf     d7,.row
        rts

; ---------------------------------------------------------------------------------------------------------------
; The palette, into the copper list: 256 colours of 24 bits, smooth steps between eight key colours (as in
; chapter 9's ch09_256colours.s), so the texture's 256 values shade smoothly.

MakePalette:
        lea     KeyColours,a0
        lea     Palette,a1
        moveq   #8-1,d7
.key:
        move.l  (a0)+,d0
        move.l  (a0),d1
        tst.w   d7
        bne.s   .notlast
        move.l  KeyColours,d1
.notlast:
        moveq   #0,d2
.step:
        moveq   #0,d6
        moveq   #16,d5
.component:
        move.l  d0,d3
        lsr.l   d5,d3
        and.w   #$ff,d3
        move.l  d1,d4
        lsr.l   d5,d4
        and.w   #$ff,d4
        sub.w   d3,d4
        muls    d2,d4
        asr.l   #5,d4
        add.w   d3,d4
        and.l   #$ff,d4
        lsl.l   d5,d4
        or.l    d4,d6
        subq.w  #8,d5
        bpl.s   .component
        move.l  d6,(a1)+
        addq.w  #1,d2
        cmp.w   #32,d2
        blo.s   .step
        dbf     d7,.key

        ; Into the copper list: for each bank of 32, the high halves, then (LOCT set) the low halves.
        lea     CopperPalette,a0
        lea     Palette,a1
        moveq   #0,d0
.bank:
        move.w  d0,d1
        ror.w   #3,d1
        or.w    #$0c20,d1
        move.w  #BPLCON3,(a0)+
        move.w  d1,(a0)+
        moveq   #4,d4                   ; the high nibbles: shift 4
        bsr.s   .registers
        or.w    #$0200,d1
        move.w  #BPLCON3,(a0)+
        move.w  d1,(a0)+
        moveq   #0,d4                   ; the low nibbles
        bsr.s   .registers
        lea     32*4(a1),a1
        addq.w  #1,d0
        cmp.w   #8,d0
        blo.s   .bank
        rts
.registers:
        move.w  #COLOR00,d2
        moveq   #32-1,d3
        move.l  a1,a2
.register:
        move.l  (a2)+,d5                ; $00RrGgBb
        lsr.l   d4,d5
        moveq   #0,d6
        move.b  d5,d6
        and.w   #$0f,d6                 ; blue
        lsr.l   #4,d5
        move.w  d5,d7
        and.w   #$f0,d7                 ; green
        or.w    d7,d6
        lsr.l   #4,d5
        and.w   #$f00,d5                ; red
        or.w    d5,d6
        move.w  d2,(a0)+
        move.w  d6,(a0)+
        addq.w  #2,d2
        dbf     d3,.register
        rts

; ---------------------------------------------------------------------------------------------------------------
; The panel: the times, in frames to two places, from the line counts.

ShowTimes:
        move.l  RenderLines,d0
        moveq   #7,d1
        bsr.s   ShowFrames
        move.l  C2PLines,d0
        moveq   #21,d1
        bsr.s   ShowFrames
        moveq   #0,d0
        move.w  FramesPerPicture,d0
        cmp.w   #99,d0
        bls.s   .fits
        moveq   #99,d0
.fits:
        divu    #10,d0                  ; tens, and the units in the top word
        lea     TimeText,a0
        move.b  #" ",(a0)
        tst.w   d0
        beq.s   .units
        add.b   #"0",d0
        move.b  d0,(a0)
.units:
        swap    d0
        add.b   #"0",d0
        move.b  d0,1(a0)
        clr.b   2(a0)
        moveq   #37,d0
        moveq   #12,d1
        bra     DrawText

; A time in lines (d0) as frames to two places, "nn.nn", at column d1 of the times' row.
ShowFrames:
        move.w  d1,-(sp)
        move.w  LinesPerFrame,d1
        mulu    #100,d0                 ; hundredths of a line (d0 is under 65536 lines)
        moveq   #0,d2
        move.w  d1,d2
        lsr.w   #1,d2
        add.l   d2,d0                   ; rounded to the nearest hundredth of a frame
        divu    d1,d0
        and.l   #$ffff,d0
        cmp.l   #9999,d0
        bls.s   .digits
        move.l  #9999,d0
.digits:
        lea     TimeText+5,a0
        clr.b   (a0)
        moveq   #5-1,d2                 ; five places, one of them the point
.digit:
        cmp.w   #2,d2
        bne.s   .figure
        move.b  #".",-(a0)
        bra.s   .next
.figure:
        divu    #10,d0
        swap    d0
        add.b   #"0",d0
        move.b  d0,-(a0)
        clr.w   d0
        swap    d0
.next:
        dbf     d2,.digit
        cmp.b   #"0",(a0)               ; no nought in front
        bne.s   .draw
        move.b  #" ",(a0)
.draw:
        move.w  (sp)+,d0
        moveq   #12,d1
        bra     DrawText

; The processor, by AttnFlags (chapter 14): a0 = its name.
WhichProcessor:
        move.l  ExecBase.w,a6
        move.w  AttnFlags(a6),d0
        lea     Name68060,a0
        btst    #AFB_68060,d0
        bne.s   .known
        lea     Name68040,a0
        btst    #AFB_68040,d0
        bne.s   .known
        lea     Name68030,a0
        btst    #AFB_68030,d0
        bne.s   .known
        lea     Name68020,a0
        btst    #AFB_68020,d0
        bne.s   .known
        lea     Name68000,a0
.known:
        rts

; Text, 8 by 8 pixels a character, into the panel. a0 = the string (ending in zero), d0 = the column (0 to 39),
; d1 = the row, in pixels.
DrawText:
        move.l  PanelStart,a1
        mulu    #ROWBYTES,d1
        add.l   d1,a1
        add.w   d0,a1
.char:
        moveq   #0,d0
        move.b  (a0)+,d0
        beq.s   .done
        sub.b   #32,d0
        and.w   #63,d0
        lsl.w   #3,d0
        lea     Font,a2
        add.w   d0,a2
        move.b  (a2)+,(a1)
        move.b  (a2)+,ROWBYTES*1(a1)
        move.b  (a2)+,ROWBYTES*2(a1)
        move.b  (a2)+,ROWBYTES*3(a1)
        move.b  (a2)+,ROWBYTES*4(a1)
        move.b  (a2)+,ROWBYTES*5(a1)
        move.b  (a2)+,ROWBYTES*6(a1)
        move.b  (a2)+,ROWBYTES*7(a1)
        addq.l  #1,a1
        bra.s   .char
.done:
        rts

        section data,data

LastPicture:    dc.l    0
LinesPerFrame:  dc.w    313             ; PAL: lines 0 to 312 (NTSC: 263, set by Init)
SwapPending:    dc.b    0
        even

; The key colours, 24 bits each: night blue, sea, ice, white, gold, flame, rose, violet.
KeyColours:
        dc.l    $000c1a40,$00127a9a,$0070d0e0,$00f4f4f0
        dc.l    $00f0c040,$00e05010,$00a01050,$00381060

; A whole turn of sine in 256 words: round(sin(2*pi*i/256) * 16384), 2.14 fixed point (16384 as 16383).
Sine:
        dc.w    0,402,804,1205,1606,2006,2404,2801
        dc.w    3196,3590,3981,4370,4756,5139,5520,5897
        dc.w    6270,6639,7005,7366,7723,8076,8423,8765
        dc.w    9102,9434,9760,10080,10394,10702,11003,11297
        dc.w    11585,11866,12140,12406,12665,12916,13160,13395
        dc.w    13623,13842,14053,14256,14449,14635,14811,14978
        dc.w    15137,15286,15426,15557,15679,15791,15893,15986
        dc.w    16069,16143,16207,16261,16305,16340,16364,16379
        dc.w    16383,16379,16364,16340,16305,16261,16207,16143
        dc.w    16069,15986,15893,15791,15679,15557,15426,15286
        dc.w    15137,14978,14811,14635,14449,14256,14053,13842
        dc.w    13623,13395,13160,12916,12665,12406,12140,11866
        dc.w    11585,11297,11003,10702,10394,10080,9760,9434
        dc.w    9102,8765,8423,8076,7723,7366,7005,6639
        dc.w    6270,5897,5520,5139,4756,4370,3981,3590
        dc.w    3196,2801,2404,2006,1606,1205,804,402
        dc.w    0,-402,-804,-1205,-1606,-2006,-2404,-2801
        dc.w    -3196,-3590,-3981,-4370,-4756,-5139,-5520,-5897
        dc.w    -6270,-6639,-7005,-7366,-7723,-8076,-8423,-8765
        dc.w    -9102,-9434,-9760,-10080,-10394,-10702,-11003,-11297
        dc.w    -11585,-11866,-12140,-12406,-12665,-12916,-13160,-13395
        dc.w    -13623,-13842,-14053,-14256,-14449,-14635,-14811,-14978
        dc.w    -15137,-15286,-15426,-15557,-15679,-15791,-15893,-15986
        dc.w    -16069,-16143,-16207,-16261,-16305,-16340,-16364,-16379
        dc.w    -16384,-16379,-16364,-16340,-16305,-16261,-16207,-16143
        dc.w    -16069,-15986,-15893,-15791,-15679,-15557,-15426,-15286
        dc.w    -15137,-14978,-14811,-14635,-14449,-14256,-14053,-13842
        dc.w    -13623,-13395,-13160,-12916,-12665,-12406,-12140,-11866
        dc.w    -11585,-11297,-11003,-10702,-10394,-10080,-9760,-9434
        dc.w    -9102,-8765,-8423,-8076,-7723,-7366,-7005,-6639
        dc.w    -6270,-5897,-5520,-5139,-4756,-4370,-3981,-3590
        dc.w    -3196,-2801,-2404,-2006,-1606,-1205,-804,-402

Name68000:      dc.b    "68000",0
Name68020:      dc.b    "68020",0
Name68030:      dc.b    "68030",0
Name68040:      dc.b    "68040",0
Name68060:      dc.b    "68060",0
AkikoText:      dc.b    "AKIKO C2P",0
NoAkikoText:    dc.b    "CPU C2P: NO AKIKO HERE",0
CachedText:     dc.b    "CPU C2P: AKIKO NOT USED ON 68030+",0
LabelsText:     dc.b    "RENDER       FR  C2P       FR  EVERY",0
        even

; An 8 by 8 font for the characters from the space to the underline (ASCII 32 to 95), made for How To Code
; v8 (it's chapter 14's): one byte a row, the top row first.
Font:
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; space
        dc.b    $10,$10,$10,$10,$10,$00,$10,$00   ; !
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; "
        dc.b    $28,$28,$7c,$28,$7c,$28,$28,$00   ; #
        dc.b    $10,$3c,$50,$38,$14,$78,$10,$00   ; $
        dc.b    $60,$64,$08,$10,$20,$4c,$0c,$00   ; %
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; &
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; '
        dc.b    $08,$10,$20,$20,$20,$10,$08,$00   ; (
        dc.b    $20,$10,$08,$08,$08,$10,$20,$00   ; )
        dc.b    $00,$10,$54,$38,$54,$10,$00,$00   ; *
        dc.b    $00,$10,$10,$7c,$10,$10,$00,$00   ; +
        dc.b    $00,$00,$00,$00,$18,$08,$10,$00   ; ,
        dc.b    $00,$00,$00,$7c,$00,$00,$00,$00   ; -
        dc.b    $00,$00,$00,$00,$00,$30,$30,$00   ; .
        dc.b    $00,$04,$08,$10,$20,$40,$00,$00   ; /
        dc.b    $38,$44,$4c,$54,$64,$44,$38,$00   ; 0
        dc.b    $10,$30,$10,$10,$10,$10,$38,$00   ; 1
        dc.b    $38,$44,$04,$08,$10,$20,$7c,$00   ; 2
        dc.b    $7c,$08,$10,$08,$04,$44,$38,$00   ; 3
        dc.b    $08,$18,$28,$48,$7c,$08,$08,$00   ; 4
        dc.b    $7c,$40,$78,$04,$04,$44,$38,$00   ; 5
        dc.b    $18,$20,$40,$78,$44,$44,$38,$00   ; 6
        dc.b    $7c,$04,$08,$10,$20,$20,$20,$00   ; 7
        dc.b    $38,$44,$44,$38,$44,$44,$38,$00   ; 8
        dc.b    $38,$44,$44,$3c,$04,$08,$30,$00   ; 9
        dc.b    $00,$30,$30,$00,$30,$30,$00,$00   ; :
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ;
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; <
        dc.b    $00,$00,$7c,$00,$7c,$00,$00,$00   ; =
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; >
        dc.b    $38,$44,$04,$08,$10,$00,$10,$00   ; ?
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; @
        dc.b    $38,$44,$44,$7c,$44,$44,$44,$00   ; A
        dc.b    $78,$44,$44,$78,$44,$44,$78,$00   ; B
        dc.b    $38,$44,$40,$40,$40,$44,$38,$00   ; C
        dc.b    $70,$48,$44,$44,$44,$48,$70,$00   ; D
        dc.b    $7c,$40,$40,$78,$40,$40,$7c,$00   ; E
        dc.b    $7c,$40,$40,$78,$40,$40,$40,$00   ; F
        dc.b    $38,$44,$40,$5c,$44,$44,$3c,$00   ; G
        dc.b    $44,$44,$44,$7c,$44,$44,$44,$00   ; H
        dc.b    $38,$10,$10,$10,$10,$10,$38,$00   ; I
        dc.b    $1c,$08,$08,$08,$08,$48,$30,$00   ; J
        dc.b    $44,$48,$50,$60,$50,$48,$44,$00   ; K
        dc.b    $40,$40,$40,$40,$40,$40,$7c,$00   ; L
        dc.b    $44,$6c,$54,$54,$44,$44,$44,$00   ; M
        dc.b    $44,$44,$64,$54,$4c,$44,$44,$00   ; N
        dc.b    $38,$44,$44,$44,$44,$44,$38,$00   ; O
        dc.b    $78,$44,$44,$78,$40,$40,$40,$00   ; P
        dc.b    $38,$44,$44,$44,$54,$48,$34,$00   ; Q
        dc.b    $78,$44,$44,$78,$50,$48,$44,$00   ; R
        dc.b    $3c,$40,$40,$38,$04,$04,$78,$00   ; S
        dc.b    $7c,$10,$10,$10,$10,$10,$10,$00   ; T
        dc.b    $44,$44,$44,$44,$44,$44,$38,$00   ; U
        dc.b    $44,$44,$44,$44,$44,$28,$10,$00   ; V
        dc.b    $44,$44,$44,$54,$54,$54,$28,$00   ; W
        dc.b    $44,$44,$28,$10,$28,$44,$44,$00   ; X
        dc.b    $44,$44,$28,$10,$10,$10,$10,$00   ; Y
        dc.b    $7c,$04,$08,$10,$20,$40,$7c,$00   ; Z
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; [
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; \
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ]
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; ^
        dc.b    $00,$00,$00,$00,$00,$00,$00,$00   ; _

        section bss,bss

ShowScreen:     ds.l    1
DrawScreen:     ds.l    1
PendingScreen:  ds.l    1
AkikoC2PReg:    ds.l    1               ; Akiko's C2P register, or 0 to use C2P8
ModeText:       ds.l    1
PanelStart:     ds.l    1
TimeStart:      ds.l    1
RenderLines:    ds.l    1
C2PLines:       ds.l    1
FramesPerPicture: ds.w  1
TimeText:       ds.b    8
Palette:        ds.l    256
Deltas:         ds.w    CHUNKYW
C2PScratch:     ds.l    ROWSTRIDE/4     ; where the first block's "last block" writes go
; These two are in any memory: fast RAM, if the machine has some.
Chunky:         ds.b    CHUNKYW*CHUNKYH
Texture:        ds.b    TEXBYTES*2

; ---------------------------------------------------------------------------------------------------------------
; The copper list, in chip RAM.

        section copper,data_c

Copper:
        dc.w    BPLCON0,$0201           ; no bitplanes until the rest is ready (without AGA, this is all);
                                        ; ECSENA on, so BRDRBLNK keeps the border black from the top
CopperAGA:
        dc.w    FMODE,$0003             ; bitplanes fetched 64 bits at a time (4x)
        dc.w    BPLCON3,$0c20           ; bank 0, LOCT clear, BRDRBLNK: the border black
        dc.w    BPLCON4,$0011
        dc.w    DIWSTRT,$2c81
        dc.w    DIWSTOP,(LASTLINE&$ff)<<8!$c1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00b8           ; 4x: five fetches of 64 pixels
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,ROWSTRIDE-ROWBYTES
        dc.w    BPL2MOD,ROWSTRIDE-ROWBYTES
CopperPalette:
        ds.w    (2+32*2)*2*8            ; filled in by MakePalette: 8 banks of 32 colours, high and low halves
        dc.w    BPLCON3,$0c20
        dc.w    (FIRSTLINE-4)<<8!$01,$fffe      ; line $28: VBlank has finished with the pointers
CopperBitplanes:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL2PTH,0,BPL2PTL,0
        dc.w    BPL3PTH,0,BPL3PTL,0
        dc.w    BPL4PTH,0,BPL4PTL,0
        dc.w    BPL5PTH,0,BPL5PTL,0
        dc.w    BPL6PTH,0,BPL6PTL,0
        dc.w    BPL7PTH,0,BPL7PTH+2,0
        dc.w    BPL8PTH,0,BPL8PTH+2,0
        dc.w    BPLCON0,$0211           ; 8 bitplanes, colour on, ECSENA (for BRDRBLNK)
        ; The panel: one bitplane, after a blank line, with colours 0 and 1 black and white for it.
        dc.w    (PANELLINE-1)<<8!$07,$fffe
        dc.w    BPLCON0,$0201
        dc.w    COLOR00,$0000,COLOR01,$0fff     ; the high halves (and so, for now, the low ones too)
        dc.w    (PANELLINE)<<8!$07,$fffe
CopperPanel:
        dc.w    BPL1PTH,0,BPL1PTL,0
        dc.w    BPL1MOD,0
        dc.w    BPLCON0,$1201
        dc.w    (LASTLINE)<<8!$07,$fffe
        dc.w    BPLCON0,$0201
        dc.w    $ffff,$fffe

        section screen,bss_c

ScreenSpace:    ds.b    SCREENBYTES*2+8
PanelSpace:     ds.b    ROWBYTES*PANELHEIGHT+8
