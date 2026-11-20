; ch12_tone.s - one of Paula's channels playing a sampled waveform: a new note from a period table every second,
; up and down a scale, with the note on show as a lit key. The sound stops cleanly as the demo ends. Left mouse
; button to leave.
;
; How To Code v8, chapter 12. CC0: use it for anything (the waveform is made from sines, below).
;
; Build and run it with F9. Set OS_FRIENDLY to 1 to keep the system running underneath: then the channel is
; first allocated from audio.device, so nothing else in the system can use it while the demo does.
;
; $VER: ch12_tone 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
NOTES           equ     8               ; keys on show, one for each note in the scale
KEYTOP          equ     80              ; where the keys are on the screen, in lines
KEYLINES        equ     96
VOLUME          equ     48              ; 0 to 64

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: the screen, then the sound.

Init:
        ; A clear screen (a section of BSS is cleared as it's loaded from Kickstart 2 on, not always before).
        lea     Screen,a0
        move.w  #ROWBYTES*HEIGHT/4-1,d0
.clear:
        clr.l   (a0)+
        dbf     d0,.clear

        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2

        ; All eight sprites at an empty sprite, so none shows even with sprite DMA on (as the system leaves it).
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
        move.w  #$f4c1,CopperWindowEnd+2        ; NTSC: 200 lines
        move.l  #PeriodsNTSC,Periods            ; and the NTSC clock's periods
.pal:
        move.l  #ToneExit,ExitHook              ; called as the demo ends

        ifne    OS_FRIENDLY
        ; With the system running, the channel is asked for first. If another program has it, the demo runs
        ; without sound rather than fight over it.
        bsr     AllocateChannel
        tst.l   d0
        beq.s   .nosound
        endc

        ; The sound, before the display starts: where the waveform is, how long it is, how loud, how fast. With
        ; its DMA off, the channel keeps all these until it's started.
        lea     CUSTOM,a5
        move.l  #Wave,AUD0LC(a5)                ; the address: chip RAM, a word boundary
        move.w  #WAVEBYTES/2,AUD0LEN(a5)        ; the length, in words
        move.w  #VOLUME,AUD0VOL(a5)
        move.l  Periods,a0
        move.w  (a0),AUD0PER(a5)                ; the first note
        clr.w   NoteIndex
        move.w  #DMAF_SETCLR!DMAF_MASTER!DMAF_AUD0,DMACON(a5)  ; and go: the waveform repeats until it's stopped
        st      Playing
.nosound:
        bsr     DrawKeys
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt: a new note once a second. Writing AUDxPER while the channel
; plays changes the pitch from the next sample on, with no click, as the waveform carries on where it was.

VBlank:
        tst.b   Playing
        beq.s   .done
        addq.w  #1,NoteFrames
        moveq   #50,d0                          ; frames in a second
        tst.b   PalDisplay
        bne.s   .pal
        moveq   #60,d0
.pal:
        cmp.w   NoteFrames,d0
        bhi.s   .done
        clr.w   NoteFrames
        ; The next note in the tune: up the scale and down again.
        move.w  TunePosition,d0
        addq.w  #1,d0
        cmp.w   #TUNELENGTH,d0
        blo.s   .inside
        moveq   #0,d0
.inside:
        move.w  d0,TunePosition
        lea     Tune,a0
        moveq   #0,d1
        move.b  (a0,d0.w),d1
        move.w  d1,NoteIndex
        add.w   d1,d1
        move.l  Periods,a0
        move.w  (a0,d1.w),AUD0PER(a5)
.done:
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the keys drawn again when the note has changed.

Frame:
        move.w  NoteIndex,d0
        cmp.w   ShownNote,d0
        beq.s   .same
        bsr     DrawKeys
.same:
        rts

; The keys: an outline for each note, and the one playing filled in. Each key is 24 pixels (3 bytes) wide, one
; every 40 pixels (5 bytes), starting 8 pixels in. Half the keys start at an odd address, so they're written a
; byte at a time: a word or a long at an odd address is an address error on a 68000 (a 68020 doesn't mind, which
; is how such a bug gets past a fast Amiga).
DrawKeys:
        move.w  NoteIndex,ShownNote
        lea     Screen+KEYTOP*ROWBYTES+1,a0
        moveq   #0,d7                           ; which key
.key:
        move.l  a0,a1
        moveq   #-1,d0                          ; the top line, full
        bsr.s   PutLine
        move.l  #$800001,d0                     ; the sides
        cmp.w   ShownNote,d7
        bne.s   .outline
        moveq   #-1,d0                          ; or all of it, for the note playing
.outline:
        move.w  #KEYLINES-3,d6
.line:
        bsr.s   PutLine
        dbf     d6,.line
        moveq   #-1,d0                          ; the bottom line
        bsr.s   PutLine
        addq.l  #5,a0
        addq.w  #1,d7
        cmp.w   #NOTES,d7
        blo.s   .key
        rts

; The low three bytes of d0 at a1, a byte at a time; a1 moves on to the next line.
PutLine:
        swap    d0
        move.b  d0,(a1)
        swap    d0
        move.w  d0,d1
        lsr.w   #8,d1
        move.b  d1,1(a1)
        move.b  d0,2(a1)
        lea     ROWBYTES(a1),a1
        rts

; ---------------------------------------------------------------------------------------------------------------
; Called by the startup as the demo ends, before it gives anything back: the channel's DMA off and its volume 0,
; so it stops at once and stays silent (taking the machine, the startup does this too, for all four channels,
; but a program that started a sound should stop it). Then, with the system running, the channel goes back to audio.device.

ToneExit:
        tst.b   Playing
        beq.s   .silent
        move.w  #DMAF_AUD0,DMACON(a5)
        move.w  #0,AUD0VOL(a5)
        sf      Playing
.silent:
        ifne    OS_FRIENDLY
        bsr     FreeChannel
        endc
        rts

        ifne    OS_FRIENDLY
; ---------------------------------------------------------------------------------------------------------------
; With the system running, audio.device owns the four channels, and other programs ask it for them. A program
; that writes to the audio registers itself must allocate its channels first, at the highest precedence
; (ADALLOC_MAXPREC), so they can't be taken away while it plays. Opening the device with a list of the channel
; combinations wanted allocates them; closing it frees them again. The allocation needs a message port, made by
; hand (CreateMsgPort is only in Kickstart 2 and later). Returns d0 = 0 if the channel couldn't be had.

AllocateChannel:
        move.l  ExecBase.w,a6
        moveq   #-1,d0
        jsr     _LVOAllocSignal(a6)
        move.b  d0,AudioSignal
        bmi     .nosignal
        lea     AudioPort,a0
        move.b  #NT_MSGPORT,LN_TYPE(a0)
        move.b  #PA_SIGNAL,MP_FLAGS(a0)
        move.b  d0,MP_SIGBIT(a0)
        move.l  OurTask,MP_SIGTASK(a0)
        lea     MP_MSGLIST(a0),a1               ; an empty list
        move.l  a1,LH_TAILPRED(a1)
        addq.l  #LH_TAIL,a1
        move.l  a1,-LH_TAIL(a1)
        clr.l   (a1)
        lea     AudioRequest,a1
        move.b  #NT_MESSAGE,LN_TYPE(a1)
        move.b  #ADALLOC_MAXPREC,LN_PRI(a1)     ; the allocation's precedence: nothing can take it from us
        move.l  a0,MN_REPLYPORT(a1)
        move.w  #ioa_SIZEOF,MN_LENGTH(a1)
        clr.w   ioa_AllocKey(a1)        ; 0: a new allocation (memory isn't always cleared on Kickstart 1.x)
        move.l  #ChannelWanted,ioa_Data(a1)
        move.l  #1,ioa_Length(a1)               ; one combination to try
        lea     AudioName(pc),a0
        moveq   #0,d0
        moveq   #0,d1
        jsr     _LVOOpenDevice(a6)
        tst.l   d0
        bne.s   .noopen
        st      AudioOpen
        moveq   #1,d0
        rts
.noopen:
        moveq   #0,d0
        move.b  AudioSignal,d0
        jsr     _LVOFreeSignal(a6)
.nosignal:
        moveq   #0,d0
        rts

FreeChannel:
        tst.b   AudioOpen
        beq.s   .done
        sf      AudioOpen
        move.l  ExecBase.w,a6
        lea     AudioRequest,a1
        jsr     _LVOCloseDevice(a6)             ; frees the channel
        moveq   #0,d0
        move.b  AudioSignal,d0
        jsr     _LVOFreeSignal(a6)
.done:
        rts

AudioName:      dc.b    "audio.device",0
ChannelWanted:  dc.b    %0001                   ; channel 0 (bits 0 to 3 are channels 0 to 3)
        even
        endc

        section data,data

; The periods for a C major scale, middle C to the C above, for a waveform of 32 samples (one cycle):
; period = clock / (32 * frequency), with the PAL clock 3546895 Hz and the NTSC clock 3579545 Hz. The NTSC
; periods are ProTracker's own: its notes are tuned for NTSC, so on PAL they play about a sixth of a semitone flat.
PeriodsPAL:     dc.w    424,377,336,317,283,252,224,212
PeriodsNTSC:    dc.w    428,381,339,320,285,254,226,214
Periods:        dc.l    PeriodsPAL

Tune:           dc.b    0,1,2,3,4,5,6,7,6,5,4,3,2,1
TUNELENGTH      equ     *-Tune
        even

NoteFrames:     dc.w    0
TunePosition:   dc.w    0
NoteIndex:      dc.w    0
ShownNote:      dc.w    -1
Playing:        dc.b    0
AudioOpen:      dc.b    0
AudioSignal:    dc.b    0
        even

        section bss,bss

AudioPort:      ds.b    MP_SIZE
AudioRequest:   ds.b    ioa_SIZEOF

; ---------------------------------------------------------------------------------------------------------------
; In chip RAM, where the chips can read them: the waveform, the copper list and the empty sprite.

        section chipdata,data_c

; One cycle of a waveform, 32 signed samples: a sine with a little of its second and third harmonics, so it sounds
; rounder than a plain sine. Made with round(100*sin(x) + 20*sin(2x) + 12*sin(3x)), x = 2*pi*i/32. It starts at 0,
; so starting it makes no click.
Wave:
        dc.b    0,34,63,86,99,104,102,96,88,80,74,67,59,49,35,19
        dc.b    0,-19,-35,-49,-59,-67,-74,-80,-88,-96,-102,-104,-99,-86,-63,-34
WAVEBYTES       equ     *-Wave

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
        dc.w    BPLCON2,$0000
        dc.w    BPL1MOD,$0000
        dc.w    BPL2MOD,$0000
        dc.w    COLOR00,$0213
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
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

        section screen,bss_c

Screen:
        ds.b    ROWBYTES*HEIGHT
