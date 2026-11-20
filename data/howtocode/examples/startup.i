; startup.i - takes the Amiga from the system for a demo, and gives it all back afterwards.
;
; How To Code v8. CC0: use it for anything.
;
; Include this at the start of a demo, after hardware.i and system.i, and write four things of your own:
;
;   Init     called once, before the display starts: build the screen, fill in the copper list
;   Frame    called once a frame from the main loop, with the display running
;   VBlank   called at the start of every frame from the interrupt: keep it short (a music replay, a counter)
;   Copper   the copper list, in a chip RAM section
;
; Each is called with a5 = CUSTOM and may change any register. A program that installs anything of its own in Init
; (another interrupt, say) puts the address of a routine that removes it in ExitHook:
;
;           move.l  #MyExit,ExitHook
;
; and the startup calls it first as the demo ends, in either way, while the machine is still the demo's. The demo
; ends when the left mouse button is pressed (or the program sets ExitNow to non-zero), and everything is put back
; as it was: the audio filter (and the power light, which shows it) as it was found, and on an AGA machine
; BPLCON4, which the system never resets itself. Taking the machine, the demo's sound stops with it (audio DMA off
; and every channel's volume 0); with the system running, the sound is the demo's own business, unless it asks the
; startup to allocate its channels (AUDIO_CHANNELS, below).
;
; AgaChips is not zero on an Amiga with the AGA chips (A1200, A4000, CD32), found by asking the chip, not the
; system, so it's right before SetPatch has run too.
;
; Settings, given before the include (each has a default):
;
;   OS_FRIENDLY   0: take the machine completely (the usual way for a demo: all of the chips are yours).
;                 1: keep the system running underneath (as How To Code v7 did): multitasking stays on, the
;                    frame comes from a system interrupt server, input goes through an input handler that keeps
;                    it from Workbench, and the blitter is shared - so dos.library can load files from any disk
;                    while the demo runs.
;   TASK_PRIORITY with OS_FRIENDLY, the demo's task priority (30: above input.device and the disks' tasks, so
;                 the demo isn't held up, but they still run whenever the demo waits for the next frame).
;   AUTOEXIT      when not 0, the demo ends by itself after this many frames (for testing).
;   NEEDS_AGA     1: the demo needs the AGA chips. On a machine without them it says so (in the CLI it was
;                 started from) and ends at once, before taking anything.
;   AUDIO_CHANNELS with OS_FRIENDLY: the channels the demo plays, as a mask (%1111 for all four), for the startup
;                 to allocate from audio.device, so no other program's sound cuts in; they're silenced and given
;                 back as the demo ends. 0 (the usual): the startup leaves sound to the demo.
;
; It works from the CLI or from an icon, on any Amiga from Kickstart 1.3 on, with any processor.

        ifnd    OS_FRIENDLY
OS_FRIENDLY     equ     0
        endc
        ifnd    TASK_PRIORITY
TASK_PRIORITY   equ     30
        endc
        ifnd    AUTOEXIT
AUTOEXIT        equ     0
        endc
        ifnd    NEEDS_AGA
NEEDS_AGA       equ     0
        endc
        ifnd    AUDIO_CHANNELS
AUDIO_CHANNELS  equ     0
        endc

; ---------------------------------------------------------------------------------------------------------------
; The blitter. With the system running it has to be shared: the system uses it too (on Kickstart 1.x even to
; read floppy disks), so own it around each piece of blitting and give it back after. Taking the machine, it's
; yours all along. Put BLIT_BEGIN before a blit (or a run of them) and BLIT_END after.
;
; BLIT_END always waits for the blitter to finish, whichever way: the processor mustn't touch what the blitter
; is still writing. On a 68000 the blitter usually finishes first anyway, so forgetting this works - until a
; faster processor overtakes the blitter and its work is overwritten (How To Code v8 found exactly this in its
; own startup demo, on a 68040).

BLIT_BEGIN      macro
        ifne    OS_FRIENDLY
        movem.l d0-d1/a0-a1/a6,-(sp)
        move.l  GfxBase,a6
        jsr     _LVOOwnBlitter(a6)
        movem.l (sp)+,d0-d1/a0-a1/a6
        endc
        jsr     WaitBlitter             ; jsr: the program's code is in another section
        endm

BLIT_END        macro
        jsr     WaitBlitter             ; jsr: the program's code is in another section
        ifne    OS_FRIENDLY
        movem.l d0-d1/a0-a1/a6,-(sp)
        move.l  GfxBase,a6
        jsr     _LVODisownBlitter(a6)
        movem.l (sp)+,d0-d1/a0-a1/a6
        endc
        endm

; ---------------------------------------------------------------------------------------------------------------

        section startup,code

Start:
        movem.l d1-d7/a0-a6,-(sp)
        move.l  ExecBase.w,a6

        ; Started from Workbench, a program is sent a message it has to take before it does anything, and give
        ; back as it ends; from the CLI there's none. Without this, starting from an icon crashes or loses memory.
        sub.l   a1,a1
        jsr     _LVOFindTask(a6)
        move.l  d0,OurTask
        move.l  d0,a4
        tst.l   pr_CLI(a4)
        bne.s   .fromcli
        lea     pr_MsgPort(a4),a0
        jsr     _LVOWaitPort(a6)
        lea     pr_MsgPort(a4),a0
        jsr     _LVOGetMsg(a6)
        move.l  d0,WorkbenchMessage
.fromcli:

        ; Open the libraries properly, with a version (33 is Kickstart 1.2), and check they opened. Never walk
        ; ExecBase's list of libraries to find one, and never use OldOpenLibrary.
        lea     GraphicsName(pc),a1
        moveq   #33,d0
        jsr     _LVOOpenLibrary(a6)
        move.l  d0,GfxBase
        beq     StartNoGraphics
        lea     IntuitionName(pc),a1
        moveq   #33,d0
        jsr     _LVOOpenLibrary(a6)
        move.l  d0,IntuitionBase
        beq     StartNoIntuition

        ; PAL or NTSC. From Kickstart 2 on, graphics.library knows (gb_DisplayFlags); before that, the vertical
        ; blank frequency exec was started with is the best there is.
        st      PalDisplay
        cmp.w   #36,LIB_VERSION(a6)
        blo.s   .oldkick
        move.l  GfxBase,a0
        move.w  gb_DisplayFlags(a0),d0
        and.w   #PAL,d0
        bne.s   .palknown
        sf      PalDisplay
        bra.s   .palknown
.oldkick:
        cmp.b   #50,VBlankFrequency(a6)
        beq.s   .palknown
        sf      PalDisplay
.palknown:

        ; Where the processor's exception vectors are. On a 68000 they're at address 0, always; a 68010 or later
        ; has a vector base register (VBR), and the system may have moved them into fast RAM, so an interrupt's
        ; address must be put at VBR+$6C, not $6C. The VBR can only be read in supervisor mode.
        moveq   #0,d0
        btst    #AFB_68010,AttnFlags+1(a6)
        beq.s   .novbr
        lea     ReadVBR(pc),a5
        jsr     _LVOSupervisor(a6)
.novbr:
        move.l  d0,VectorBase

        ; The AGA chips or not, and the audio filter as it is, to put back as the demo ends (bit 1 of CIA-A's
        ; port A: 1 is the filter off and the power light dimmed).
        lea     CUSTOM,a5
        bsr     StartupIsAGA
        move.b  d0,AgaChips
        move.b  CIAA+CIAPRA,d0
        and.b   #2,d0
        move.b  d0,OldFilter
        ifne    NEEDS_AGA
        tst.b   AgaChips
        beq     StartNeedsAGA
        endc

        ; Show nothing: LoadView(0) puts the display back to plain settings (no AGA modes or interlace left
        ; on), and the two WaitTOFs let both fields of an interlaced view finish.
        move.l  GfxBase,a6
        move.l  gb_ActiView(a6),OldView
        sub.l   a1,a1
        jsr     _LVOLoadView(a6)
        jsr     _LVOWaitTOF(a6)
        jsr     _LVOWaitTOF(a6)

        lea     CUSTOM,a5

        ifeq    OS_FRIENDLY
; ---- Taking the machine -----------------------------------------------------------------------------------------

        ; The blitter is ours from here on, and no other task runs.
        jsr     _LVOOwnBlitter(a6)
        jsr     _LVOWaitBlit(a6)
        move.l  ExecBase.w,a6
        jsr     _LVOForbid(a6)

        ; Note the chips' settings to give back: reading gives the bits that are on, and with bit 15 set,
        ; writing them back switches the same bits on again.
        move.w  DMACONR(a5),d0
        or.w    #DMAF_SETCLR,d0
        move.w  d0,OldDMACON
        move.w  INTENAR(a5),d0
        or.w    #INTF_SETCLR,d0
        move.w  d0,OldINTENA
        move.w  ADKCONR(a5),d0
        or.w    #$8000,d0
        move.w  d0,OldADKCON

        ; Every interrupt and every kind of DMA off, in the vertical blank (so no sprite is cut off half drawn).
        bsr     WaitVerticalBlank
        move.w  #$7fff,INTENA(a5)
        move.w  #$7fff,INTREQ(a5)
        move.w  #$7fff,INTREQ(a5)
        move.w  #$7fff,DMACON(a5)

        ; Our own level 3 interrupt, for the start of each frame.
        move.l  VectorBase,a0
        move.l  $6c(a0),OldLevel3
        lea     Level3(pc),a1
        move.l  a1,$6c(a0)

        ; The blitter, so Init can use it; then the program's own setting up.
        move.w  #DMAF_SETCLR!DMAF_MASTER!DMAF_BLITTER,DMACON(a5)
        jsr     Init
        lea     CUSTOM,a5

        ; The copper list starts at the next frame, with the bitplanes and the copper (sprites stay off), and
        ; the interrupt for each frame.
        bsr     WaitVerticalBlank
        move.l  #Copper,COP1LC(a5)
        move.w  d0,COPJMP1(a5)          ; writing anything here starts the copper list
        move.w  #DMAF_SETCLR!DMAF_MASTER!DMAF_RASTER!DMAF_COPPER!DMAF_BLITTER,DMACON(a5)
        move.w  #INTF_SETCLR!INTF_INTEN!INTF_VERTB,INTENA(a5)

        else
; ---- Keeping the system running ---------------------------------------------------------------------------------

        ; A high priority instead of Forbid: the demo isn't held up, but whenever it waits for the next frame
        ; the system's tasks run - input.device, the disks' tasks, the file systems - so files can be loaded.
        move.l  ExecBase.w,a6
        move.l  OurTask,a1
        moveq   #TASK_PRIORITY,d0
        jsr     _LVOSetTaskPri(a6)
        move.b  d0,OldPriority

        ; The DMA the demo needs, as it was, to give back as the demo ends. On Kickstart 3, LoadView(0) has
        ; switched the bitplanes off (Kickstart 1.3 leaves them on), so they have to be switched on: never count
        ; on DMA being left as your own machine leaves it.
        move.w  DMACONR(a5),d0
        and.w   #DMAF_RASTER!DMAF_COPPER!DMAF_BLITTER!DMAF_SPRITE,d0
        move.w  d0,OldDMACON
        ; Sprite DMA off, as it is when the machine's taken (the mouse pointer would otherwise be fetched from
        ; wherever the sprite pointers were left): a demo that wants sprites switches it on in Init. It's the
        ; vertical blank just now (after the WaitTOFs), so no sprite is cut off half drawn.
        move.w  #DMAF_SPRITE,DMACON(a5)

        ; A signal, for the interrupt server to wake the main loop with at the start of each frame.
        moveq   #-1,d0
        jsr     _LVOAllocSignal(a6)
        move.b  d0,FrameSignal
        bmi     StartNoSignal
        moveq   #0,d1
        bset    d0,d1
        move.l  d1,FrameMask

        ; Input goes to our input handler before Intuition sees it, so clicks and keys don't fall through to
        ; Workbench and its windows while the demo runs.
        bsr     AddInputHandler
        tst.l   d0
        beq     StartNoInput

        ifne    AUDIO_CHANNELS
        bsr     AllocateAudio
        endc

        ; The frame's interrupt: a server on the system's vertical blank chain.
        moveq   #INTB_VERTB,d0
        lea     VBlankServer,a1
        jsr     _LVOAddIntServer(a6)

        lea     CUSTOM,a5
        jsr     Init
        lea     CUSTOM,a5

        ; Our copper list in place of the system's. It stays until something asks the system to rebuild the
        ; display (a requester, say); the system's own copper list comes back as the demo ends.
        jsr     WaitFrame
        move.l  #Copper,COP1LC(a5)
        move.w  d0,COPJMP1(a5)
        move.w  #DMAF_SETCLR!DMAF_MASTER!DMAF_RASTER!DMAF_COPPER!DMAF_BLITTER,DMACON(a5)

        endc

; ---- The main loop: once a frame, until the left mouse button is pressed ----------------------------------------

MainLoop:
        jsr     WaitFrame
        lea     CUSTOM,a5
        jsr     Frame
        tst.b   ExitNow
        bne.s   .out
        ifne    AUTOEXIT
        cmp.l   #AUTOEXIT,Frames
        bhs.s   .out
        endc
        bsr     MouseClicked
        beq.s   MainLoop
.out:
        ; The program's own things first, while everything it set up is still running.
        move.l  ExitHook,d0
        beq.s   .nohook
        move.l  d0,a0
        lea     CUSTOM,a5
        jsr     (a0)
.nohook:
        lea     CUSTOM,a5

        ifeq    OS_FRIENDLY
; ---- Giving the machine back ------------------------------------------------------------------------------------

        ; The demo's sound stops with it: all four channels are the demo's.
        move.w  #DMAF_AUDIO,DMACON(a5)
        moveq   #0,d0
        move.w  d0,AUD0LC+8(a5)         ; AUDxVOL: 8 on from each channel's AUDxLC
        move.w  d0,AUD1LC+8(a5)
        move.w  d0,AUD2LC+8(a5)
        move.w  d0,AUD3LC+8(a5)

        bsr     WaitBlitter
        bsr     WaitVerticalBlank
        move.w  #$7fff,INTENA(a5)
        move.w  #$7fff,INTREQ(a5)
        move.w  #$7fff,INTREQ(a5)
        move.w  #$7fff,DMACON(a5)

        move.l  VectorBase,a0
        move.l  OldLevel3,$6c(a0)

        ; The system's own copper list, then its settings, as they were.
        move.l  GfxBase,a6
        move.l  gb_copinit(a6),COP1LC(a5)
        move.w  d0,COPJMP1(a5)
        move.w  #$7fff,ADKCON(a5)
        move.w  OldADKCON,ADKCON(a5)
        move.w  OldDMACON,DMACON(a5)
        move.w  OldINTENA,INTENA(a5)

        move.l  ExecBase.w,a6
        jsr     _LVOPermit(a6)
        move.l  GfxBase,a6
        jsr     _LVODisownBlitter(a6)

        else
; ---- Leaving the system as it was -------------------------------------------------------------------------------

        bsr     WaitBlitter
        move.l  ExecBase.w,a6
        moveq   #INTB_VERTB,d0
        lea     VBlankServer,a1
        jsr     _LVORemIntServer(a6)
        ifne    AUDIO_CHANNELS
        bsr     FreeAudio
        endc
        lea     CUSTOM,a5
        move.l  GfxBase,a6
        move.l  gb_copinit(a6),COP1LC(a5)
        move.w  d0,COPJMP1(a5)
        ; The bitplanes, copper, blitter and sprites back as they were: off again if they were off, on if on.
        move.w  #DMAF_RASTER!DMAF_COPPER!DMAF_BLITTER!DMAF_SPRITE,d0
        move.w  OldDMACON,d1
        not.w   d1
        and.w   d1,d0
        move.w  d0,DMACON(a5)
        move.w  OldDMACON,d0
        or.w    #DMAF_SETCLR,d0
        move.w  d0,DMACON(a5)
        bsr     RemoveInputHandler
StartNoInput:
        move.l  ExecBase.w,a6
        moveq   #0,d0
        move.b  FrameSignal,d0
        jsr     _LVOFreeSignal(a6)
StartNoSignal:
        move.l  ExecBase.w,a6
        move.l  OurTask,a1
        moveq   #0,d0
        move.b  OldPriority,d0
        jsr     _LVOSetTaskPri(a6)

        endc

        ; The audio filter as it was found.
        tst.b   OldFilter
        bne.s   .filteroff
        bclr    #1,CIAA+CIAPRA
        bra.s   .filterdone
.filteroff:
        bset    #1,CIAA+CIAPRA
.filterdone:
        ; On AGA, BPLCON4 as the system expects it: the system never sets it itself, and a demo that changed it
        ; would leave Workbench's mouse pointer in the demo's colours.
        tst.b   AgaChips
        beq.s   .notaga
        move.w  #$0011,CUSTOM+BPLCON4
.notaga:

        ; The system's view again, and Intuition told to make its display afresh.
        move.l  GfxBase,a6
        move.l  OldView,a1
        jsr     _LVOLoadView(a6)
        jsr     _LVOWaitTOF(a6)
        jsr     _LVOWaitTOF(a6)
        move.l  IntuitionBase,a6
        jsr     _LVORethinkDisplay(a6)

StartCloseLibraries:
        move.l  ExecBase.w,a6
        move.l  IntuitionBase,a1
        jsr     _LVOCloseLibrary(a6)
StartNoIntuition:
        move.l  ExecBase.w,a6
        move.l  GfxBase,a1
        jsr     _LVOCloseLibrary(a6)
StartNoGraphics:
        ; Workbench's message goes back last, with multitasking off, so Workbench can't unload the program
        ; before it has finished.
        move.l  WorkbenchMessage,d0
        beq.s   StartDone
        move.l  ExecBase.w,a6
        jsr     _LVOForbid(a6)
        move.l  WorkbenchMessage,a1
        jsr     _LVOReplyMsg(a6)
StartDone:
        movem.l (sp)+,d1-d7/a0-a6
        move.l  ReturnCode,d0           ; 0: all went well (10 or more would stop a script)
        rts

        ifne    NEEDS_AGA
; ---------------------------------------------------------------------------------------------------------------
; Without the AGA chips: say so in the CLI the demo was started from (from an icon there's nowhere to say it), and
; end with a warning (5), which a script carries on after.

StartNeedsAGA:
        move.l  #5,ReturnCode
        tst.l   WorkbenchMessage
        bne     StartCloseLibraries
        move.l  ExecBase.w,a6
        lea     StartupDosName(pc),a1
        moveq   #33,d0
        jsr     _LVOOpenLibrary(a6)
        tst.l   d0
        beq     StartCloseLibraries
        move.l  d0,a6
        jsr     _LVOOutput(a6)
        move.l  d0,d1
        beq.s   .nowhere
        move.l  #NeedsAGAText,d2
        moveq   #NeedsAGATextEnd-NeedsAGAText,d3
        jsr     _LVOWrite(a6)
.nowhere:
        move.l  a6,a1
        move.l  ExecBase.w,a6
        jsr     _LVOCloseLibrary(a6)
        bra     StartCloseLibraries
        endc

; ---------------------------------------------------------------------------------------------------------------
; Is this an AGA machine? Returns d0 = 1 if so, 0 if not; a5 = CUSTOM.
;
; Lisa, AGA's video chip, answers a read of DENISEID with $F8 in the low byte (ECS's Denise answers $FC). The
; first Denise has no such register, and a read gives whatever is on the bus, which could be $F8 by chance: so
; it's read many times, as a chip that's really there gives the same answer every time.

StartupIsAGA:
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
; Read in supervisor mode by Supervisor(): the VBR, in d0. It's MOVEC VBR,D0, written as words so any assembler
; set for the 68000 takes it; it's only run on a 68010 or later.

ReadVBR:
        dc.l    $4e7a0801               ; movec vbr,d0
        rte

; ---------------------------------------------------------------------------------------------------------------
; Waits for the next frame. Returns with a5 = CUSTOM.

WaitFrame:
        ifeq    OS_FRIENDLY
        move.l  Frames,d0
.wait:
        cmp.l   Frames,d0
        beq.s   .wait
        else
        ; Sleep until the interrupt server signals: the system's other tasks run meanwhile.
        move.l  ExecBase.w,a6
        move.l  FrameMask,d0
        jsr     _LVOWait(a6)
        endc
        lea     CUSTOM,a5
        rts

; ---------------------------------------------------------------------------------------------------------------
; Whether the left mouse button has been pressed: not zero (and the Z flag clear) if so.

MouseClicked:
        ifeq    OS_FRIENDLY
        moveq   #0,d0
        btst    #CIAB_GAMEPORT0,CIAA+CIAPRA
        seq     d0
        else
        move.b  ButtonPressed,d0
        endc
        tst.b   d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; Waits for a line in the vertical blank, before the display starts (line 16, on PAL and NTSC alike).

WaitVerticalBlank:
.wait:
        move.l  VPOSR(a5),d0
        and.l   #$1ff00,d0
        cmp.l   #16<<8,d0
        bne.s   .wait
        rts

; ---------------------------------------------------------------------------------------------------------------
; Waits until the blitter has finished. The first read of DMACONR can be wrong on the first Agnus chips, so
; the busy bit is read twice.

WaitBlitter:
        btst    #DMAB_BLTDONE-8,DMACONR(a5)
.wait:
        btst    #DMAB_BLTDONE-8,DMACONR(a5)
        bne.s   .wait
        rts

        ifeq    OS_FRIENDLY
; ---------------------------------------------------------------------------------------------------------------
; The level 3 interrupt: the start of each frame (and the copper and blitter, which aren't switched on here).

Level3:
        movem.l d0-d7/a0-a6,-(sp)
        lea     CUSTOM,a5
        move.w  INTREQR(a5),d0          ; read as a word: the chips' registers are words
        btst    #INTB_VERTB,d0
        beq.s   .notframe
        addq.l  #1,Frames
        jsr     VBlank
        lea     CUSTOM,a5
.notframe:
        ; Only the vertical blank is acknowledged: it's all this handles. A demo that switches on the copper's or
        ; the blitter's interrupt puts its own handler in front of this one (see chapter 5), and acknowledges
        ; those itself. Acknowledged twice: on a fast processor, the first write may not have reached the chips
        ; by the RTE, and the interrupt would come straight back.
        move.w  #INTF_VERTB,INTREQ(a5)
        move.w  #INTF_VERTB,INTREQ(a5)
        movem.l (sp)+,d0-d7/a0-a6
        rte

        else
; ---------------------------------------------------------------------------------------------------------------
; The vertical blank server, called by the system at the start of each frame. It may change d0, d1, a0, a1, a5
; and a6 and must keep the rest; it returns with the Z flag set, so the servers after it run too.

VBlankServerCode:
        movem.l d2-d7/a2-a4,-(sp)
        addq.l  #1,Frames
        lea     CUSTOM,a5
        jsr     VBlank
        move.l  ExecBase.w,a6
        move.l  OurTask,a1
        move.l  FrameMask,d0
        jsr     _LVOSignal(a6)
        movem.l (sp)+,d2-d7/a2-a4
        moveq   #0,d0
        rts

; ---------------------------------------------------------------------------------------------------------------
; The input handler: called by input.device with a0 = the list of input events, before Intuition sees them.
; Mouse and keyboard events are noted and then made into nothing (IECLASS_NULL), so they don't reach Workbench;
; the rest (disks being changed, for one) carry on. Returns the list in d0.

InputHandlerCode:
        move.l  a0,d0
.event:
        move.b  ie_Class(a0),d1
        cmp.b   #IECLASS_RAWMOUSE,d1
        bne.s   .notmouse
        cmp.w   #IECODE_LBUTTON,ie_Code(a0)
        bne.s   .swallow
        st      ButtonPressed
        bra.s   .swallow
.notmouse:
        cmp.b   #IECLASS_RAWKEY,d1
        bne.s   .next
        move.w  ie_Code(a0),LastKey
.swallow:
        move.b  #IECLASS_NULL,ie_Class(a0)
.next:
        move.l  (a0),d1
        move.l  d1,a0
        bne.s   .event
        rts

; ---------------------------------------------------------------------------------------------------------------
; Adds the input handler, through input.device. Its message port is made by hand, as CreateMsgPort is only in
; Kickstart 2 and later. Returns d0 = 0 if it couldn't be added.

AddInputHandler:
        move.l  ExecBase.w,a6
        moveq   #-1,d0
        jsr     _LVOAllocSignal(a6)
        move.b  d0,PortSignal
        bmi     .failed
        lea     InputPort,a0
        move.b  #NT_MSGPORT,LN_TYPE(a0)
        move.b  #PA_SIGNAL,MP_FLAGS(a0)
        move.b  d0,MP_SIGBIT(a0)
        move.l  OurTask,MP_SIGTASK(a0)
        lea     MP_MSGLIST(a0),a1       ; an empty list: its head points at its tail, its tail-predecessor at
        move.l  a1,LH_TAILPRED(a1)      ; its head
        addq.l  #LH_TAIL,a1
        move.l  a1,-LH_TAIL(a1)
        clr.l   (a1)
        lea     InputRequest,a1
        move.b  #NT_MESSAGE,LN_TYPE(a1)
        move.l  a0,MN_REPLYPORT(a1)
        move.w  #IOSTD_SIZE,MN_LENGTH(a1)
        lea     InputName(pc),a0
        moveq   #0,d0
        moveq   #0,d1
        jsr     _LVOOpenDevice(a6)
        tst.l   d0
        bne.s   .nodevice
        lea     InputRequest,a1
        move.w  #IND_ADDHANDLER,IO_COMMAND(a1)
        move.l  #InputHandler,IO_DATA(a1)
        jsr     _LVODoIO(a6)
        moveq   #1,d0
        rts
.nodevice:
        moveq   #0,d0
        move.b  PortSignal,d0
        jsr     _LVOFreeSignal(a6)
.failed:
        moveq   #0,d0
        rts

        ifne    AUDIO_CHANNELS
; ---------------------------------------------------------------------------------------------------------------
; The demo's channels, allocated from audio.device at the highest precedence, so no other program can take them
; while the demo plays. Opening the device with a mask to try allocates them; it doesn't wait if another program
; has them (the demo then plays anyway, and may be cut in on).

AllocateAudio:
        move.l  ExecBase.w,a6
        lea     StartupAudioRequest,a1
        move.b  #NT_MESSAGE,LN_TYPE(a1)
        move.b  #ADALLOC_MAXPREC,LN_PRI(a1)
        move.l  #InputPort,MN_REPLYPORT(a1)
        move.w  #ioa_SIZEOF,MN_LENGTH(a1)
        move.b  #ADIOF_NOWAIT,IO_FLAGS(a1)
        clr.w   ioa_AllocKey(a1)        ; 0: a new allocation (memory isn't always cleared on Kickstart 1.x)
        move.l  #StartupAudioMask,ioa_Data(a1)
        move.l  #1,ioa_Length(a1)
        lea     StartupAudioName(pc),a0
        moveq   #0,d0
        moveq   #0,d1
        jsr     _LVOOpenDevice(a6)
        tst.l   d0
        seq     StartupAudioOpen
        rts

; The channels silenced, then given back by closing the device.
FreeAudio:
        tst.b   StartupAudioOpen
        beq.s   .none
        lea     CUSTOM,a5
        move.w  #AUDIO_CHANNELS&DMAF_AUDIO,DMACON(a5)
        moveq   #0,d0                   ; each allocated channel's volume (AUDxVOL, 8 on from its AUDxLC) to 0
        ifne    AUDIO_CHANNELS&1
        move.w  d0,AUD0LC+8(a5)
        endc
        ifne    AUDIO_CHANNELS&2
        move.w  d0,AUD1LC+8(a5)
        endc
        ifne    AUDIO_CHANNELS&4
        move.w  d0,AUD2LC+8(a5)
        endc
        ifne    AUDIO_CHANNELS&8
        move.w  d0,AUD3LC+8(a5)
        endc
        move.l  ExecBase.w,a6
        lea     StartupAudioRequest,a1
        jsr     _LVOCloseDevice(a6)
.none:
        rts

        endc

RemoveInputHandler:
        move.l  ExecBase.w,a6
        lea     InputRequest,a1
        move.w  #IND_REMHANDLER,IO_COMMAND(a1)
        move.l  #InputHandler,IO_DATA(a1)
        jsr     _LVODoIO(a6)
        lea     InputRequest,a1
        jsr     _LVOCloseDevice(a6)
        moveq   #0,d0
        move.b  PortSignal,d0
        jsr     _LVOFreeSignal(a6)
        rts

        endc

GraphicsName:   dc.b    "graphics.library",0
IntuitionName:  dc.b    "intuition.library",0
InputName:      dc.b    "input.device",0
StartupDosName: dc.b    "dos.library",0
StartupAudioName: dc.b  "audio.device",0
NeedsAGAText:   dc.b    "This demo needs an Amiga with the AGA chips: an A1200, A4000 or CD32.",10
NeedsAGATextEnd:
ServerName:     dc.b    "How To Code v8 startup",0
        even

; ---------------------------------------------------------------------------------------------------------------

        section startupdata,data

GfxBase:        dc.l    0
IntuitionBase:  dc.l    0
OurTask:        dc.l    0
WorkbenchMessage: dc.l  0
OldView:        dc.l    0
VectorBase:     dc.l    0
OldLevel3:      dc.l    0
Frames:         dc.l    0               ; how many frames since the display started
ExitHook:       dc.l    0               ; set by the program: a routine to call first as the demo ends
FrameMask:      dc.l    0
ReturnCode:     dc.l    0
LastKey:        dc.w    0               ; with OS_FRIENDLY, the last raw key code (with $80 set as it's let go)
OldDMACON:      dc.w    0
OldINTENA:      dc.w    0
OldADKCON:      dc.w    0
PalDisplay:     dc.b    0               ; not zero on a PAL display
ExitNow:        dc.b    0               ; set this to end the demo
ButtonPressed:  dc.b    0
FrameSignal:    dc.b    0
PortSignal:     dc.b    0
OldPriority:    dc.b    0
AgaChips:       dc.b    0               ; not zero on an Amiga with the AGA chips
OldFilter:      dc.b    0
StartupAudioOpen:      dc.b    0
StartupAudioMask: dc.b  AUDIO_CHANNELS
        even

        ifne    OS_FRIENDLY
VBlankServer:
        dc.l    0,0                     ; the node's links
        dc.b    NT_INTERRUPT,0          ; its type, and its priority on the chain
        dc.l    ServerName
        dc.l    0,VBlankServerCode      ; is_Data, is_Code
InputHandler:
        dc.l    0,0
        dc.b    NT_INTERRUPT,100        ; ahead of Intuition (priority 50), so it sees the input first
        dc.l    ServerName
        dc.l    0,InputHandlerCode
        endc

        section startupbss,bss

InputPort:      ds.b    MP_SIZE
InputRequest:   ds.b    IOSTD_SIZE
StartupAudioRequest: ds.b ioa_SIZEOF
