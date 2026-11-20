; ch12_tiny_player.s - a very small music player, playing a short tune on all four channels from the vertical
; blank: bass, a lead, a chord arpeggio and drums. Four bars on screen show each channel's volume, and a row of
; steps shows the beat. Left mouse button to leave.
;
; How To Code v8, chapter 12. CC0: use it for anything. The tune and every sound in it are made here: the
; waveforms and drums are worked out by the code in Init, and the tune is in the lists at the end.
;
; This is a player to learn from, not a tracker replayer: each channel reads its own list of notes and lengths.
; It shows what every replayer has to do: a period table for PAL and NTSC, volume envelopes, starting a sample
; with the waits Paula needs, a one-shot sample stopped by pointing the channel at silence, and a steady tempo
; on NTSC as well as PAL.
;
; Build and run it with F9. Set OS_FRIENDLY to 1 to keep the system running underneath: then the four channels
; are allocated from audio.device first.
;
; $VER: ch12_tiny_player 8.0 (20.11.2026)

OS_FRIENDLY     equ     0

        include "hardware.i"
        include "system.i"
        include "startup.i"

WIDTH           equ     320
HEIGHT          equ     256
ROWBYTES        equ     WIDTH/8
BARTOP          equ     60              ; the volume bars: where they start, how tall, how far apart
BARLINES        equ     12
BARGAP          equ     28
STEPTOP         equ     190             ; the row of steps

SPEED           equ     6               ; ticks (frames at 50 Hz) to a row: 6 is 125 BPM, four rows to a beat
DMA_WAIT_LINES  equ     10              ; see StartNotes
DRUM_PERIOD     equ     256             ; the drums play at 3546895/256 = 13855 samples a second (PAL)

; The audio registers, from a channel's AUDxLC.
AUDLEN          equ     AUD0LEN-AUD0LC
AUDPER          equ     AUD0PER-AUD0LC
AUDVOL          equ     AUD0VOL-AUD0LC

; What the player keeps for each channel.
CH_EVENTS       equ     0               ; l: the next event in its list
CH_LIST         equ     4               ; l: the start of the list, to go back to
CH_ROWS         equ     8               ; w: rows until the next event
CH_VOLUME       equ     10              ; w: the volume now, times 256 (so it can fall by fractions)
CH_KIND         equ     12              ; w: KIND_TONE, KIND_ARP or KIND_DRUM
CH_WAVE         equ     14              ; l: the waveform, for tones
CH_WAVELEN      equ     18              ; w: its length in words
CH_STARTVOL     equ     20              ; w: the volume a note starts at, times 256
CH_DECAY        equ     22              ; w: how much it falls each tick
CH_FLOOR        equ     24              ; w: and how low it falls
CH_CHORD        equ     26              ; l: for the arpeggio, the chord's three notes
CH_ARPSTEP      equ     30              ; w: which of them plays now
CH_NEWLC        equ     32              ; l: for a note starting: its sample,
CH_NEWLEN       equ     36              ; w: its length
CH_NEWPER       equ     38              ; w: and its period
CH_SIZE         equ     40

KIND_TONE       equ     0
KIND_ARP        equ     1
KIND_DRUM       equ     2

; Events: a note (or a drum, or a chord) and how many rows it lasts.
OFF             equ     $fe             ; silence: the volume to 0
LOOP            equ     $ff             ; back to the start of the list

        section code,code

; ---------------------------------------------------------------------------------------------------------------
; Once, before the display starts: the screen, the sounds, the player.

Init:
        lea     Screen,a0
        move.w  #ROWBYTES*HEIGHT/4-1,d0
.clear:
        clr.l   (a0)+
        dbf     d0,.clear

        move.l  #Screen,d0
        move.w  d0,CopperBitplane+6
        swap    d0
        move.w  d0,CopperBitplane+2
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
        move.w  #$f4c1,CopperWindowEnd+2
        move.l  #PeriodsNTSC,Periods
        move.w  #DRUM_PERIOD*3579545/3546895,DrumPeriod  ; the same pitch on NTSC
.pal:
        bsr     MakeSounds
        bsr     DrawSteps

        move.l  #PlayerExit,ExitHook

        ifne    OS_FRIENDLY
        bsr     AllocateChannels
        tst.l   d0
        beq.s   .nosound                        ; another program has them: no music, rather than a fight
        endc

        ; All four channels silent and stopped, then the player goes.
        lea     CUSTOM,a5
        move.w  #DMAF_AUDIO,DMACON(a5)
        moveq   #0,d0
        move.w  d0,AUD0VOL(a5)
        move.w  d0,AUD0VOL+AUD_SIZE(a5)
        move.w  d0,AUD0VOL+AUD_SIZE*2(a5)
        move.w  d0,AUD0VOL+AUD_SIZE*3(a5)
        move.w  #SPEED-1,Tick                   ; so the first row is read on the first tick
        st      Playing
.nosound:
        rts

; ---------------------------------------------------------------------------------------------------------------
; The sounds, worked out in chip RAM: waveforms of one cycle for the tones, and three drums.

MakeSounds:
        ; The bass: a triangle of 64 samples. Played from the same period table as the 32-sample waveforms, it
        ; sounds an octave lower: twice the samples at the same rate take twice as long.
        lea     Triangle64,a0
        moveq   #-120,d0
        moveq   #32-1,d1
.up:
        move.b  d0,(a0)+
        addq.w  #8,d0                           ; -120 up to 128, in steps of 8 ...
        cmp.w   #127,d0
        ble.s   .upok
        moveq   #127,d0
.upok:
        dbf     d1,.up
        moveq   #32-1,d1
.down:
        move.b  d0,(a0)+
        subq.w  #8,d0                           ; ... and back down
        dbf     d1,.down

        ; The lead: a pulse, a quarter of the cycle high (96 for a quarter and -32 for the rest, so it averages 0:
        ; a waveform that doesn't thump as it starts and stops). The arpeggio: a square, half high.
        lea     Pulse32,a0
        lea     Square32,a1
        moveq   #32-1,d1
.pulse:
        moveq   #-32,d0
        cmp.w   #24,d1
        blo.s   .low
        moveq   #96,d0
.low:
        move.b  d0,(a0)+
        moveq   #-64,d0
        cmp.w   #16,d1
        blo.s   .low2
        moveq   #64,d0
.low2:
        move.b  d0,(a1)+
        dbf     d1,.pulse

        ; The kick drum: a triangle wave falling from about 150 Hz to 50 Hz, fading out. d2 is the phase and d3
        ; how far it moves each sample, both times 256.
        lea     Kick,a0
        move.w  #KICKBYTES,FadeLength
        moveq   #0,d2
        move.l  #709*256,d3                     ; 150 Hz at 13855 samples a second: 150 * 65536 / 13855
        move.w  #KICKBYTES-1,d1
.kick:
        move.l  d2,d0
        lsr.l   #8,d0                           ; the phase, 0 to 65535
        lsr.w   #8,d0                           ; 0 to 255
        cmp.w   #128,d0                         ; a triangle: up for half the cycle, down for the other
        blo.s   .rising
        not.b   d0
.rising:
        add.w   d0,d0
        sub.w   #128,d0                         ; -128 to 126
        bsr.s   Fade
        move.b  d0,(a0)+
        add.l   d3,d2
        sub.l   #59,d3                          ; falling to about 50 Hz by the end
        dbf     d1,.kick

        ; The snare and the hi-hat: noise, fading out, from a 16-bit shift register (a "linear feedback shift
        ; register": the same noise every time, with no tables).
        move.w  #$ace1,d4                       ; its starting value: anything but 0
        lea     Snare,a0
        move.w  #SNAREBYTES,FadeLength
        move.w  #SNAREBYTES-1,d1
        bsr.s   Noise
        lea     HiHat,a0
        move.w  #HIHATBYTES,FadeLength
        move.w  #HIHATBYTES-1,d1
        bsr.s   Noise
        clr.w   Silence
        rts

; d1+1 bytes of fading noise at a0.
Noise:
.byte:
        lsr.w   #1,d4                           ; shift; if a 1 fell out, flip the bits that feed back
        bcc.s   .same
        eor.w   #$b400,d4
.same:
        move.b  d4,d0
        ext.w   d0
        bsr.s   Fade
        move.b  d0,(a0)+
        dbf     d1,.byte
        rts

; The sample in d0 (-128 to 127) faded by how far through the sound it is: d1 counts down to 0 from FadeLength-1,
; so the sound fades in a straight line, to nothing at the end.
Fade:
        move.w  d1,-(sp)
        muls    d1,d0
        divs    FadeLength,d0
        move.w  (sp)+,d1
        rts

; ---------------------------------------------------------------------------------------------------------------
; At the start of every frame, from the interrupt: one tick of the player.

VBlank:
        tst.b   Playing
        beq     .done
        ; On NTSC there are 60 frames a second; the tune is written for 50 ticks a second (as a tracker module
        ; is), so every sixth frame is skipped and the tempo is the same as on PAL.
        tst.b   PalDisplay
        bne.s   .tick
        addq.w  #1,NtscFrames
        cmp.w   #6,NtscFrames
        blo.s   .tick
        clr.w   NtscFrames
        rts
.tick:
        moveq   #0,d7                           ; the channels with a note starting this tick
        addq.w  #1,Tick
        cmp.w   #SPEED,Tick
        blo.s   .notrow
        clr.w   Tick
        addq.w  #1,Row
        bsr     ReadRow
.notrow:
        tst.w   d7
        beq.s   .nostart
        bsr     StartNotes
.nostart:
        bsr     Envelopes
.done:
        rts

; A new row: each channel's event, when its last one has run its rows. Sets the channels starting in d7.
ReadRow:
        lea     Channels,a2
        moveq   #0,d6                           ; channel number
.channel:
        subq.w  #1,CH_ROWS(a2)
        bgt     .next
        move.l  CH_EVENTS(a2),a0
        cmp.b   #LOOP,(a0)
        bne.s   .event
        move.l  CH_LIST(a2),a0
.event:
        moveq   #0,d0
        move.b  (a0)+,d0                        ; the note
        moveq   #0,d1
        move.b  (a0)+,d1                        ; and its length
        move.w  d1,CH_ROWS(a2)
        move.l  a0,CH_EVENTS(a2)
        cmp.b   #OFF,d0
        bne.s   .note
        clr.w   CH_VOLUME(a2)
        bra.s   .next
.note:
        move.w  CH_STARTVOL(a2),CH_VOLUME(a2)
        bset    d6,d7
        cmp.w   #KIND_DRUM,CH_KIND(a2)
        beq.s   .drum
        cmp.w   #KIND_ARP,CH_KIND(a2)
        bne.s   .tone
        ; A chord: the arpeggio starts on its first note.
        lsl.w   #2,d0
        lea     Chords,a0
        add.w   d0,a0
        move.l  a0,CH_CHORD(a2)
        clr.w   CH_ARPSTEP(a2)
        move.b  (a0),d0
.tone:
        move.l  CH_WAVE(a2),CH_NEWLC(a2)
        move.w  CH_WAVELEN(a2),CH_NEWLEN(a2)
        add.w   d0,d0
        move.l  Periods,a0
        move.w  (a0,d0.w),CH_NEWPER(a2)
        bra.s   .next
.drum:
        mulu    #6,d0
        lea     Drums,a0
        add.w   d0,a0
        move.l  (a0)+,CH_NEWLC(a2)
        move.w  (a0),CH_NEWLEN(a2)
        move.w  DrumPeriod,CH_NEWPER(a2)
.next:
        lea     CH_SIZE(a2),a2
        addq.w  #1,d6
        cmp.w   #4,d6
        blo     .channel
        rts

; ---------------------------------------------------------------------------------------------------------------
; Starting new notes on the channels in d7 (bit 0 for channel 0, and so on), the way Paula needs:
;
; 1. Stop the channels' DMA. A channel only notices at the end of the sample it's playing, and it may be one
;    sample from the end of a pair, so it can take two periods of the old note to stop. Start it again before
;    then and it never stopped: it carries on through the old sample, and the new note is lost. The longest
;    period here is 847 (the bass's C), so two are 1694 colour clocks; a line is 227 of them (227.5 on NTSC),
;    so waiting for DMA_WAIT_LINES (10) new lines is at least nine whole lines, 2043 colour clocks: enough for
;    any period up to 1021, and ProTracker's longest is 907. Count lines with the beam, never with a dbf loop:
;    a loop's time depends on the processor, and on a fast one it's over before Paula has noticed anything.
; 2. Give each channel its new sample, length and period, and start them all together.
; 3. Once a channel has started, it has taken its sample's address and length into its own copies, and asks
;    for its audio interrupt to say so (the request is there in INTREQR whether the interrupt is enabled or
;    not). Then AUDxLC and AUDxLEN can be changed for what follows: for a drum, a word of silence, so the drum
;    plays once and the channel then repeats nothing. The request comes within a line or so; a limit of
;    DMA_WAIT_LINES is there in case it never comes (if the channel didn't restart).

StartNotes:
        move.w  d7,DMACON(a5)                   ; 1: the channels' DMA off (bit 15 clear: clear these bits)
        moveq   #DMA_WAIT_LINES,d0
        bsr     WaitLines

        lea     Channels,a2                     ; 2: the new samples
        lea     AUD0LC(a5),a1
        moveq   #0,d6
.set:
        btst    d6,d7
        beq.s   .notset
        move.l  CH_NEWLC(a2),(a1)
        move.w  CH_NEWLEN(a2),AUDLEN(a1)
        move.w  CH_NEWPER(a2),AUDPER(a1)
.notset:
        lea     CH_SIZE(a2),a2
        lea     AUD_SIZE(a1),a1
        addq.w  #1,d6
        cmp.w   #4,d6
        blo.s   .set

        move.w  d7,d5                           ; their audio interrupt requests: INTF_AUD0 is bit 7
        lsl.w   #7,d5
        move.w  d5,INTREQ(a5)                   ; cleared, so a new one shows the start
        move.w  d5,INTREQ(a5)
        move.w  d7,d0
        or.w    #DMAF_SETCLR,d0
        move.w  d0,DMACON(a5)                   ; and go

        moveq   #DMA_WAIT_LINES,d2              ; 3: wait for every channel to ask
        move.b  VHPOSR(a5),d1
.ask:
        move.w  INTREQR(a5),d0
        and.w   d5,d0
        cmp.w   d5,d0
        beq.s   .asked
        cmp.b   VHPOSR(a5),d1
        beq.s   .ask
        move.b  VHPOSR(a5),d1
        subq.w  #1,d2
        bne.s   .ask
.asked:
        move.w  d5,INTREQ(a5)
        move.w  d5,INTREQ(a5)

        lea     Channels,a2                     ; then what the drums go on to: silence
        lea     AUD0LC(a5),a1
        moveq   #0,d6
.repeat:
        btst    d6,d7
        beq.s   .notdrum
        cmp.w   #KIND_DRUM,CH_KIND(a2)
        bne.s   .notdrum
        move.l  #Silence,(a1)
        move.w  #1,AUDLEN(a1)
.notdrum:
        lea     CH_SIZE(a2),a2
        lea     AUD_SIZE(a1),a1
        addq.w  #1,d6
        cmp.w   #4,d6
        blo.s   .repeat
        rts

; Waits until the beam has started d0 new lines. Any processor, any speed: the beam is the clock.
WaitLines:
.line:
        move.b  VHPOSR(a5),d1                   ; the line the beam is on (its low 8 bits)
.same:
        cmp.b   VHPOSR(a5),d1
        beq.s   .same
        subq.w  #1,d0
        bne.s   .line
        rts

; Each tick: every channel's volume falls towards its floor, and the arpeggio moves to the chord's next note.
; Changing AUDxPER and AUDxVOL while a channel plays needs no waiting: they're used from the next sample on.
Envelopes:
        lea     Channels,a2
        lea     AUD0LC(a5),a1
        moveq   #4-1,d6
.channel:
        move.w  CH_VOLUME(a2),d0
        sub.w   CH_DECAY(a2),d0
        cmp.w   CH_FLOOR(a2),d0
        bge.s   .above
        move.w  CH_FLOOR(a2),d0
        tst.w   CH_VOLUME(a2)                   ; (a note that's off stays off)
        bne.s   .above
        moveq   #0,d0
.above:
        move.w  d0,CH_VOLUME(a2)
        lsr.w   #8,d0
        move.w  d0,AUDVOL(a1)
        cmp.w   #KIND_ARP,CH_KIND(a2)
        bne.s   .next
        move.l  CH_CHORD(a2),a0
        move.w  CH_ARPSTEP(a2),d0
        moveq   #0,d1
        move.b  (a0,d0.w),d1
        add.w   d1,d1
        move.l  Periods,a0
        move.w  (a0,d1.w),AUDPER(a1)
        addq.w  #1,d0
        cmp.w   #3,d0
        blo.s   .step
        moveq   #0,d0
.step:
        move.w  d0,CH_ARPSTEP(a2)
.next:
        lea     CH_SIZE(a2),a2
        lea     AUD_SIZE(a1),a1
        dbf     d6,.channel
        rts

; ---------------------------------------------------------------------------------------------------------------
; Once a frame: the volume bars, and the step the tune is on.

Frame:
        lea     Channels,a2
        lea     Screen+BARTOP*ROWBYTES+4,a0     ; 32 pixels in
        moveq   #4-1,d7
.bar:
        move.w  CH_VOLUME(a2),d0
        lsr.w   #6,d0                           ; volume 0 to 64 -> 0 to 256 pixels
        move.l  a0,a1
        moveq   #8-1,d6                         ; 8 longs: 256 pixels
.long:
        moveq   #-1,d1                          ; a whole long, 32 pixels ...
        sub.w   #32,d0
        bge.s   .put
        moveq   #0,d1                           ; ... or part of one, or none
        cmp.w   #-32,d0
        ble.s   .put
        moveq   #-1,d1
        move.w  d0,d2
        neg.w   d2                              ; 32 - the pixels wanted: shift that many off the right
        lsl.l   d2,d1
.put:
        move.w  #BARLINES-1,d5
        move.l  a1,a3
.line:
        move.l  d1,(a3)
        lea     ROWBYTES(a3),a3
        dbf     d5,.line
        addq.l  #4,a1
        dbf     d6,.long
        lea     BARGAP*ROWBYTES(a0),a0
        lea     CH_SIZE(a2),a2
        dbf     d7,.bar

        move.w  Row,d0
        cmp.w   ShownRow,d0
        beq.s   .samestep
        bsr.s   DrawSteps
.samestep:
        rts

; Sixteen steps, one a row, the one playing lit: 16 pixels wide each, 2 bytes, starting 32 pixels in.
DrawSteps:
        move.w  Row,ShownRow
        move.w  Row,d0
        and.w   #15,d0
        lea     Screen+STEPTOP*ROWBYTES+4,a0
        moveq   #0,d7
.step:
        move.w  #%0111111111111110,d1           ; lit: a block
        cmp.w   d0,d7
        beq.s   .lit
        move.w  #%0000000110000000,d1           ; not: a dot
.lit:
        move.l  a0,a1
        moveq   #8-1,d6
.line:
        move.w  d1,(a1)
        lea     ROWBYTES(a1),a1
        dbf     d6,.line
        addq.l  #2,a0
        addq.w  #1,d7
        cmp.w   #16,d7
        blo.s   .step
        rts

; ---------------------------------------------------------------------------------------------------------------
; Called by the startup as the demo ends. The player is stopped first, so a vertical blank coming after this
; can't start a note again; then the channels go silent, and with the system running, back to audio.device.

PlayerExit:
        sf      Playing
        move.w  #DMAF_AUDIO,DMACON(a5)
        moveq   #0,d0
        move.w  d0,AUD0VOL(a5)
        move.w  d0,AUD0VOL+AUD_SIZE(a5)
        move.w  d0,AUD0VOL+AUD_SIZE*2(a5)
        move.w  d0,AUD0VOL+AUD_SIZE*3(a5)
        ifne    OS_FRIENDLY
        bsr     FreeChannels
        endc
        rts

        ifne    OS_FRIENDLY
; ---------------------------------------------------------------------------------------------------------------
; The four channels from audio.device, at the highest precedence, so nothing can take them while the demo plays
; (see ch12_tone.s). Returns d0 = 0 if they couldn't be had.

AllocateChannels:
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
        lea     MP_MSGLIST(a0),a1
        move.l  a1,LH_TAILPRED(a1)
        addq.l  #LH_TAIL,a1
        move.l  a1,-LH_TAIL(a1)
        clr.l   (a1)
        lea     AudioRequest,a1
        move.b  #NT_MESSAGE,LN_TYPE(a1)
        move.b  #ADALLOC_MAXPREC,LN_PRI(a1)
        move.l  a0,MN_REPLYPORT(a1)
        move.w  #ioa_SIZEOF,MN_LENGTH(a1)
        clr.w   ioa_AllocKey(a1)        ; 0: a new allocation (memory isn't always cleared on Kickstart 1.x)
        move.l  #ChannelsWanted,ioa_Data(a1)
        move.l  #1,ioa_Length(a1)
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

FreeChannels:
        tst.b   AudioOpen
        beq.s   .done
        sf      AudioOpen
        move.l  ExecBase.w,a6
        lea     AudioRequest,a1
        jsr     _LVOCloseDevice(a6)
        moveq   #0,d0
        move.b  AudioSignal,d0
        jsr     _LVOFreeSignal(a6)
.done:
        rts

AudioName:      dc.b    "audio.device",0
ChannelsWanted: dc.b    %1111                   ; all four
        even
        endc

        section data,data

; The periods for a waveform of 32 samples, C3 (130.8 Hz) to A5 (880 Hz): period = clock / (32 * frequency).
; A5 is the highest a 32-sample waveform can go, as the period mustn't be below 124.
PeriodsPAL:
        dc.w    847,800,755,713,673,635,599,566,534,504,476,449
        dc.w    424,400,377,356,336,317,300,283,267,252,238,224
        dc.w    212,200,189,178,168,159,150,141,133,126
PeriodsNTSC:
        dc.w    855,807,762,719,679,641,605,571,539,508,480,453
        dc.w    428,404,381,360,339,320,302,285,269,254,240,226
        dc.w    214,202,190,180,170,160,151,143,135,127
Periods:        dc.l    PeriodsPAL
DrumPeriod:     dc.w    DRUM_PERIOD

; The notes, as numbers into the period table (N for note, s for sharp).
NC3     equ     0
ND3     equ     2
NE3     equ     4
NF3     equ     5
NG3     equ     7
NA3     equ     9
NB3     equ     11
NC4     equ     12
ND4     equ     14
NE4     equ     16
NF4     equ     17
NG4     equ     19
NA4     equ     21
NB4     equ     23
NC5     equ     24
ND5     equ     26
NE5     equ     28
NF5     equ     29
NG5     equ     31
NA5     equ     33

; The drums: sample, length in words.
KICK    equ     0
SNARE   equ     1
HAT     equ     2
Drums:
        dc.l    Kick
        dc.w    KICKBYTES/2
        dc.l    Snare
        dc.w    SNAREBYTES/2
        dc.l    HiHat
        dc.w    HIHATBYTES/2

; The chords the arpeggio plays: three notes each (and a byte to keep them four apart).
AMINOR  equ     0
FMAJOR  equ     1
CMAJOR  equ     2
GMAJOR  equ     3
Chords:
        dc.b    NA4,NC5,NE5,0
        dc.b    NF4,NA4,NC5,0
        dc.b    NC5,NE5,NG5,0
        dc.b    NG4,NB4,ND5,0

; The tune: A minor, F, C, G, eight bars of sixteen rows. Each list is note, rows; they loop on their own, and
; each is a whole number of bars, so they stay together.
BassList:
        ; (the bass's waveform is twice as long, so these sound an octave lower than they're written)
        dc.b    NA3,2,NA3,2,NA4,2,NA3,2,NA3,2,NA4,2,NA3,2,NE4,2
        dc.b    NF3,2,NF3,2,NF4,2,NF3,2,NF3,2,NF4,2,NF3,2,NC4,2
        dc.b    NC3,2,NC3,2,NC4,2,NC3,2,NC3,2,NC4,2,NC3,2,NG3,2
        dc.b    NG3,2,NG3,2,NG4,2,NG3,2,NG3,2,NG4,2,NG3,2,ND4,2
        dc.b    LOOP
LeadList:
        dc.b    NE5,4,NC5,2,NA4,2,NE5,4,ND5,2,NC5,2
        dc.b    NC5,4,NA4,2,NF4,2,NA4,4,NC5,2,ND5,2
        dc.b    NE5,4,NG5,4,NE5,2,ND5,2,NC5,4
        dc.b    ND5,6,NB4,2,NG4,4,OFF,4
        dc.b    NA5,4,NG5,2,NE5,2,NC5,4,NE5,4
        dc.b    NF5,4,NE5,2,NC5,2,NA4,4,NC5,4
        dc.b    NG5,4,NE5,2,NG5,2,NA5,4,NG5,4
        dc.b    ND5,4,NG5,4,NB4,4,OFF,4
        dc.b    LOOP
ArpList:
        dc.b    AMINOR,16,FMAJOR,16,CMAJOR,16,GMAJOR,16
        dc.b    LOOP
DrumList:
        dc.b    KICK,2,HAT,2,SNARE,2,HAT,2,KICK,2,KICK,2,SNARE,2,HAT,2
        dc.b    LOOP
        even

; The channels: 0 and 3 come out on the left, 1 and 2 on the right. Each starts with 1 row to go, so the first
; row reads its first event.
Channels:
        ; channel 0, left: the bass
        dc.l    BassList,BassList
        dc.w    1,0,KIND_TONE
        dc.l    Triangle64
        dc.w    64/2,52*256,48,30*256
        dc.l    0
        dc.w    0
        dc.l    0
        dc.w    0,0
        ; channel 1, right: the lead
        dc.l    LeadList,LeadList
        dc.w    1,0,KIND_TONE
        dc.l    Pulse32
        dc.w    32/2,44*256,160,22*256
        dc.l    0
        dc.w    0
        dc.l    0
        dc.w    0,0
        ; channel 2, right: the drums
        dc.l    DrumList,DrumList
        dc.w    1,0,KIND_DRUM
        dc.l    0
        dc.w    0,64*256,3*256,0
        dc.l    0
        dc.w    0
        dc.l    0
        dc.w    0,0
        ; channel 3, left: the arpeggio
        dc.l    ArpList,ArpList
        dc.w    1,0,KIND_ARP
        dc.l    Square32
        dc.w    32/2,20*256,0,20*256
        dc.l    0
        dc.w    0
        dc.l    0
        dc.w    0,0

Tick:           dc.w    0
Row:            dc.w    -1              ; the first row read is row 0
ShownRow:       dc.w    -1
NtscFrames:     dc.w    0
FadeLength:     dc.w    1
Playing:        dc.b    0
AudioOpen:      dc.b    0
AudioSignal:    dc.b    0
        even

        section bss,bss

AudioPort:      ds.b    MP_SIZE
AudioRequest:   ds.b    ioa_SIZEOF

; ---------------------------------------------------------------------------------------------------------------
; In chip RAM, where Paula and the other chips can read them.

KICKBYTES       equ     2048
SNAREBYTES      equ     2048
HIHATBYTES      equ     512

        section sounds,bss_c

Triangle64:     ds.b    64
Pulse32:        ds.b    32
Square32:       ds.b    32
Kick:           ds.b    KICKBYTES
Snare:          ds.b    SNAREBYTES
HiHat:          ds.b    HIHATBYTES
Silence:        ds.w    1               ; a word of 0: what a one-shot sample goes on to repeat

        section chipdata,data_c

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
        dc.w    COLOR00,$0112
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
        ; Each bar its own colour, set a line above it ($2c is the first line of the screen).
        dc.w    (($2c+BARTOP-1)<<8)!$07,$fffe,COLOR01,$0e64            ; bass
        dc.w    (($2c+BARTOP+BARGAP-1)<<8)!$07,$fffe,COLOR01,$0fd4     ; lead
        dc.w    (($2c+BARTOP+BARGAP*2-1)<<8)!$07,$fffe,COLOR01,$0aaa   ; drums
        dc.w    (($2c+BARTOP+BARGAP*3-1)<<8)!$07,$fffe,COLOR01,$04ce   ; arpeggio
        dc.w    (($2c+STEPTOP-1)<<8)!$07,$fffe,COLOR01,$0fff           ; the steps
        dc.w    $ffff,$fffe

NoSprite:
        dc.l    0,0

        section screen,bss_c

Screen:
        ds.b    ROWBYTES*HEIGHT
