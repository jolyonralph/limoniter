; ch03_generated_code.s - three of the general guidelines in one small demo: memory allocated rather than assumed
; (the screen in chip RAM, on a boundary of the program's choosing), code written at run time with the processor's
; caches cleared before it runs, and everything given back. The picture is diagonal stripes, drawn each frame by
; a routine the program writes for itself as it starts. Left mouse button to leave.
;
; How To Code v8, chapter 3. CC0: use it for anything.
;
; Build and run it with F9. Set OS_FRIENDLY to 1 to keep the system running underneath.
;
; $VER: ch03_generated_code 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256             ; PAL; NTSC shows the top 200 lines
ROWBYTES        equ     WIDTH/8
SCREENSIZE      equ     ROWBYTES*HEIGHT
ALIGN           equ     65536           ; where the screen starts: on a multiple of this (a power of two)
CODESIZE        equ     HEIGHT*(ROWBYTES/4+1)*2+2   ; each row: 10 moves and a rotate, a word each; then RTS

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts.

Init:
        ; Whatever is allocated here is freed by ExitFree as the demo ends, even if something fails part way.
        move.l  #ExitFree,ExitHook
        move.l  ExecBase.w,a6

        ; The screen: chip RAM, because the chips read it, and cleared. It starts on a 64K boundary, as v7's
        ; example did, so it never crosses one: the high word of its bitplane pointer is the same for every line,
        ; and a scroller would only need to change the low word. ALIGN-1 bytes more than it needs, so that a
        ; start on a multiple of ALIGN is always inside the block. (AllocMem's own blocks are only sure to start
        ; on a multiple of 8.) That spare 64K is a lot on a 512K machine: the chapter shows a cheaper way.
        move.l  #SCREENSIZE+ALIGN-1,d0
        move.l  #MEMF_CHIP!MEMF_CLEAR,d1
        jsr     _LVOAllocMem(a6)
        move.l  d0,ScreenBlock          ; the block's own address: what FreeMem needs
        beq     .failed
        add.l   #ALIGN-1,d0             ; up to the next multiple of ALIGN
        and.l   #-ALIGN,d0
        move.l  d0,Screen

        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        ; All eight sprites at an empty sprite: with the system running, sprite DMA is on, and a sprite pointer
        ; left unset shows whatever it happens to point at.
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

        ; The drawing routine, written here rather than kept in the program: 256 rows of ten MOVE.Ls and a
        ; ROL.L (about 5.5K) from a loop of a few dozen bytes. It's code, not data for the chips, so any memory
        ; will do (MEMF_ANY: fast RAM if there is any, where it runs fastest).
        move.l  #CODESIZE,d0
        moveq   #MEMF_ANY,d1
        jsr     _LVOAllocMem(a6)
        move.l  d0,DrawCode
        beq.s   .failed
        move.l  d0,a0
        move.w  TemplateMove,d2         ; the instructions' own words, as the assembler made them
        move.w  TemplateRotate,d3
        move.w  #HEIGHT-1,d1
.row:
        moveq   #ROWBYTES/4-1,d0
.move:
        move.w  d2,(a0)+
        dbf     d0,.move
        move.w  d3,(a0)+
        dbf     d1,.row
        move.w  TemplateReturn,(a0)+

        ; The new code is in memory, but perhaps not yet where the processor fetches instructions from: clear
        ; the caches before running it. Leave this out and it works on a 68000 (no caches) and fails on a 68040.
        jsr     ClearCaches
        lea     CUSTOM,a5
        rts

.failed:
        ; Not enough memory: say so by ending at once, with no bitplanes showing.
        move.w  #$0200,CopperPlanes+2
        st      ExitNow
        lea     CUSTOM,a5
        rts

; The instructions the drawing routine is made of. Never run here: only copied.
TemplateMove:   move.l  d0,(a0)+        ; 32 pixels of the pattern
TemplateRotate: rol.l   #1,d0           ; one pixel along for the next row: the stripes slope
TemplateReturn: rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the stripes drawn afresh, one pixel along from last frame's.

Frame:
        move.l  DrawCode,d1
        beq.s   .none
        move.l  d1,a1
        move.l  Screen,a0
        move.l  #$f0f0f0f0,d0
        move.l  Frames,d1
        rol.l   d1,d0                   ; ROL takes the count modulo 64: fine for a repeating pattern
        jsr     (a1)
.none:
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt. Nothing to do.

VBlank:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Clears the processor's caches after code has been written to memory, on any Amiga. On Kickstart 2.04 (V37) and
; later, exec's CacheClearU does it for every processor, and for any cache an accelerator adds. Kickstart 1.x
; hasn't CacheClearU (calling it would jump to whatever lies before exec's table of functions), so there it's done
; by hand: nothing on a 68000 or 68010, which have no cache to clear; on a 68020 or 68030, the "clear instruction
; cache" bit of the CACR, as the Hardware Reference Manual shows. Kickstart 1.x doesn't support the 68040 or 68060.
; Keeps every register but d0, d1, a0, a1.

ClearCaches:
        move.l  a6,-(sp)
        move.l  ExecBase.w,a6
        cmp.w   #37,LIB_VERSION(a6)
        blo.s   .kick1
        jsr     _LVOCacheClearU(a6)
        bra.s   .done
.kick1:
        btst    #AFB_68020,AttnFlags+1(a6)
        beq.s   .done
        move.l  a5,-(sp)
        lea     ClearInstructionCache(pc),a5
        jsr     _LVOSupervisor(a6)
        move.l  (sp)+,a5
.done:
        move.l  (sp)+,a6
        rts

; Run in supervisor mode by Supervisor(), on a 68020 or 68030 only. MOVEC written as words, so any assembler set
; for the 68000 takes it.
ClearInstructionCache:
        dc.l    $4e7a0002               ; movec cacr,d0
        or.w    #$0008,d0               ; CACRF_ClearI
        dc.l    $4e7b0002               ; movec d0,cacr
        rte

; ---------------------------------------------------------------------------------------------------------------
; Called by the startup as the demo ends: frees what Init allocated, with the sizes it was allocated with.

ExitFree:
        move.l  ExecBase.w,a6
        move.l  DrawCode,d0
        beq.s   .nocode
        move.l  d0,a1
        move.l  #CODESIZE,d0
        jsr     _LVOFreeMem(a6)
        clr.l   DrawCode
.nocode:
        move.l  ScreenBlock,d0
        beq.s   .noscreen
        move.l  d0,a1
        move.l  #SCREENSIZE+ALIGN-1,d0
        jsr     _LVOFreeMem(a6)
        clr.l   ScreenBlock
.noscreen:
        rts

        section data,data

ScreenBlock:    dc.l    0               ; the screen's block, as AllocMem gave it
Screen:         dc.l    0               ; the screen itself: the block's first multiple of ALIGN
DrawCode:       dc.l    0               ; the routine written by Init

; ---------------------------------------------------------------------------------------------------------------
; The copper list, in chip RAM. It sets every display register the picture depends on, rather than trusting
; what was there before.

        section copper,data_c

Copper:
        dc.w    FMODE,$0000             ; AGA: plain fetches, as OCS and ECS have (they ignore these)
        dc.w    BPLCON3,$0c00
        dc.w    DIWSTRT,$2c81           ; a 320 by 256 window, the usual PAL screen
        dc.w    DIWSTOP,$2cc1
        dc.w    DDFSTRT,$0038
        dc.w    DDFSTOP,$00d0
CopperPlanes:
        dc.w    BPLCON0,$1200           ; one bitplane, colour on
        dc.w    BPLCON1,$0000
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0123
        dc.w    COLOR01,$0fc4
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
        dc.w    $ffff,$fffe             ; the end: wait for a line that never comes

NoSprite:
        dc.l    0,0
