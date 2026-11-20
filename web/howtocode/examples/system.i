; system.i - the parts of the Amiga's operating system the startup uses, by name.
;
; How To Code v8. CC0: use it for anything.
;
; Each value is the NDK's (checked against the NDK 3.2's own include files), so a program built with these
; works the same as one built with the NDK's includes. If you have the NDK, you can include its files instead.

ExecBase        equ     4               ; the one fixed address: where exec.library's base is kept

; ---- exec.library, from its base in a6
_LVOSupervisor  equ     -30
_LVODisable     equ     -120
_LVOEnable      equ     -126
_LVOForbid      equ     -132
_LVOPermit      equ     -138
_LVOAddIntServer equ    -168
_LVORemIntServer equ    -174
_LVOAllocMem    equ     -198
_LVOFreeMem     equ     -210
_LVOFindTask    equ     -294
_LVOSetTaskPri  equ     -300
_LVOWait        equ     -318
_LVOSignal      equ     -324
_LVOAllocSignal equ     -330
_LVOFreeSignal  equ     -336
_LVOGetMsg      equ     -372
_LVOReplyMsg    equ     -378
_LVOWaitPort    equ     -384
_LVOCloseLibrary equ    -414
_LVOOpenDevice  equ     -444
_LVOCloseDevice equ     -450
_LVODoIO        equ     -456
_LVOOpenResource equ    -498
_LVOOpenLibrary equ     -552
_LVOCacheClearU equ     -636            ; V37 (Kickstart 2.04) and later

; ---- cia.resource (ciaa.resource and ciab.resource), from its base in a6
_LVOAddICRVector equ    -6
_LVORemICRVector equ    -12
_LVOAbleICR     equ     -18
_LVOSetICR      equ     -24

; ---- graphics.library
_LVOLoadView    equ     -222
_LVOWaitBlit    equ     -228
_LVOWaitTOF     equ     -270
_LVOOwnBlitter  equ     -456
_LVODisownBlitter equ   -462

; ---- intuition.library
_LVORethinkDisplay equ  -390

; ---- dos.library
_LVOOpen        equ     -30
_LVOClose       equ     -36
_LVORead        equ     -42
_LVOWrite       equ     -48
_LVOOutput      equ     -60

; ---- AllocMem's kinds of memory
MEMF_ANY        equ     0
MEMF_PUBLIC     equ     1
MEMF_CHIP       equ     2
MEMF_FAST       equ     4
MEMF_CLEAR      equ     $10000

; ---- In a library's base
LIB_VERSION     equ     20              ; UWORD

; ---- In ExecBase
AttnFlags       equ     296             ; UWORD: which processor and maths chip
VBlankFrequency equ     530             ; UBYTE: 50 or 60, as the system was started
ex_EClockFrequency equ  568             ; ULONG: the CIAs' clock (V36 on; on 1.3, 709379 PAL or 715909 NTSC)
AFB_68010       equ     0               ; AttnFlags' bits: set for a 68010 or anything later
AFB_68020       equ     1
AFB_68030       equ     2
AFB_68040       equ     3
AFB_68060       equ     7

; ---- In GfxBase
gb_ActiView     equ     34              ; the View on show
gb_copinit      equ     38              ; the system's own first copper list
gb_DisplayFlags equ     206             ; UWORD
PAL             equ     4               ; in gb_DisplayFlags: a PAL display (right from V36 on)

; ---- In a Process
pr_MsgPort      equ     92
pr_CLI          equ     172             ; 0 when started from Workbench

; ---- Nodes, lists, ports and messages
LN_TYPE         equ     8
LN_PRI          equ     9
LN_NAME         equ     10
LH_TAIL         equ     4
LH_TAILPRED     equ     8
LH_TYPE         equ     12
NT_INTERRUPT    equ     2
NT_MSGPORT      equ     4
NT_MESSAGE      equ     5
MP_FLAGS        equ     14
MP_SIGBIT       equ     15
MP_SIGTASK      equ     16
MP_MSGLIST      equ     20
MP_SIZE         equ     34
PA_SIGNAL       equ     0
MN_REPLYPORT    equ     14
MN_LENGTH       equ     18

; ---- IO requests
IO_COMMAND      equ     28
IO_FLAGS        equ     30
IO_ERROR        equ     31
IO_SIZE         equ     32
IO_DATA         equ     40
IOSTD_SIZE      equ     48

; ---- audio.device (devices/audio.i)
ioa_AllocKey    equ     32
ioa_Data        equ     34              ; at OpenDevice: the channel masks to try, one byte each
ioa_Length      equ     38              ; and how many of them
ioa_SIZEOF      equ     68
ADIOF_NOWAIT    equ     $40             ; in IO_FLAGS: fail at once rather than wait for the channels
ADALLOC_MAXPREC equ     127             ; LN_PRI: the highest precedence, which no other program can steal from

; ---- Interrupts
IS_DATA         equ     14
IS_CODE         equ     18
IS_SIZE         equ     22

; ---- input.device and its events
IND_ADDHANDLER  equ     9
IND_REMHANDLER  equ     10
ie_Class        equ     4
ie_Code         equ     6
IECLASS_NULL    equ     0
IECLASS_RAWKEY  equ     1
IECLASS_RAWMOUSE equ    2
IECODE_LBUTTON  equ     $68             ; and with $80 set, its release
IECODE_UP_PREFIX equ    $80
