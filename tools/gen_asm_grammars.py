#!/usr/bin/env python3
"""Writes the 6809, 8080, 65816, ARM, PDP-11, VAX, AVR and PIC assembler grammars into data/syntaxes/, from one layout,
so they can't drift apart. Usage: python3 -I tools/gen_asm_grammars.py data/syntaxes

The instruction lists are from those processors' published instruction sets, written from memory (see MECHANICS.md).
"""
import sys, os, re
OUT = sys.argv[1]

def words(text):
    return "|".join(re.escape(w) for w in sorted(set(text.lower().split()), key=lambda w: (-len(w), w)))


def q(pattern):
    """A pattern as a single-quoted YAML string."""
    return "'" + pattern.replace("'", "''") + "'"

def grammar(key, name, exts, header, scope, *, branch, instruction, directive, registers, numbers,
            comments=(";",), label=r"^\s*[A-Za-z_.@$][\w.@$]*:{1,2}", bare_label=r"^[A-Za-z_.@$][\w.@$]*",
            strings=('"', "'"), word=r"[\w.$@]", extra_main="", extra_operands=""):
    lines = ["%YAML 1.2", "---"]
    lines += ["# " + l for l in header.strip().splitlines()]
    lines += [f"name: {name}", f"file_extensions: [{', '.join(chr(34) + e + chr(34) if e.isdigit() else e for e in exts)}]", f"scope: source.assembly.{scope}", "variables:",
              f"  branch: (?:{branch})", f"  instruction: (?:{instruction})", f"  directive: (?:{directive})",
              f"  edge_before: {q('(?<!' + word + ')')}", f"  edge_after: {q('(?!' + word + ')')}", "contexts:", "  main:", "    - include: comment"]
    lines += [extra_main] if extra_main else []
    lines += [f"    - match: {q(label)}", f"      scope: entity.name.label.{scope}",
              f"    - match: '(?i){{{{edge_before}}}}(?:{{{{directive}}}}){{{{edge_after}}}}'", f"      scope: keyword.control.directive.{scope}", "      push: operands",
              f"    - match: '(?i){{{{edge_before}}}}(?:{{{{branch}}}}){{{{edge_after}}}}'", f"      scope: keyword.control.branch.{scope}", "      push: operands",
              f"    - match: '(?i){{{{edge_before}}}}(?:{{{{instruction}}}}){{{{edge_after}}}}'", f"      scope: support.function.instruction.{scope}", "      push: operands",
              *([f"    - match: {q(bare_label)}", f"      scope: entity.name.label.{scope}"] if bare_label else []),
              "    - match: '[A-Za-z_.@][\\w.@$]*'", f"      scope: variable.function.macro.{scope}", "      push: operands",
              "  operands:", "    - match: '\\n'", "      pop: true", "    - include: comment"]
    lines += [extra_operands] if extra_operands else []
    lines += ["    - include: strings", "    - include: numbers",
              f"    - match: {q('(?i){{edge_before}}(?:' + registers + '){{edge_after}}')}", f"      scope: variable.language.register.{scope}",
              "    - match: '[+\\-*/&|^~<>=!#@]'", f"      scope: keyword.operator.{scope}", "  comment:"]
    for c in comments:
        if c == "/*":
            lines += ["    - match: '/\\*'", "      push:", f"        - meta_scope: comment.block.{scope}", "        - match: '\\*/'", "          pop: true"]
        else:
            lines += [f"    - match: {q(c + '.*$')}", f"      scope: comment.line.{scope}"]
    lines += ["  strings:"]
    for qq in strings:
        if qq == '"':
            lines += ["    - match: '\"[^\"\\n]*\"?'", f"      scope: string.quoted.double.{scope}"]
        elif qq == "'":
            lines += ['    - match: "\'[^\'\\\\n]*\'?"', f"      scope: string.quoted.single.{scope}"]
        else:
            lines += [f"    - match: {q(qq)}", f"      scope: string.quoted.other.{scope}"]
    lines += ["  numbers:", f"    - match: {q(numbers)}", f"      scope: constant.numeric.{scope}"]
    open(os.path.join(OUT, key + ".sublime-syntax"), "w", newline="\n").write("\n".join(lines) + "\n")

hexnum = r"(?i)\$[0-9a-f]+|%[01]+|0x[0-9a-f]+|(?<![\w.$@])[0-9][0-9a-f]*h(?![\w.])|(?<![\w.$@])\d+(?![\w.])"

grammar("assembly-6809", "Assembly 6809", ["6809", "s09", "a09", "asm09", "6309"], """
Written for Limoniter from the Motorola 6809 instruction set (and the Hitachi 6309's commonest additions), with
the directives of the common assemblers (Motorola, lwasm, asm6809). Branches include the long LBxx forms.""",
    "6809",
    branch=words("bcc bcs beq bge bgt bhi bhs ble blo bls blt bmi bne bpl bra brn bsr bvc bvs jmp jsr rti rts lbcc lbcs lbeq lbge lbgt lbhi lbhs lble lblo lbls lblt lbmi lbne lbpl lbra lbrn lbsr lbvc lbvs"),
    instruction=words("abx adca adcb adda addb addd anda andb andcc asla aslb asl asra asrb asr bita bitb clra clrb clr cmpa cmpb cmpd cmps cmpu cmpx cmpy coma comb com cwai daa deca decb dec eora eorb exg inca incb inc lda ldb ldd lds ldu ldx ldy leas leau leax leay lsla lslb lsl lsra lsrb lsr mul nega negb neg nop ora orb orcc pshs pshu puls pulu rola rolb rol rora rorb ror sbca sbcb sex sta stb std sts stu stx sty suba subb subd swi swi2 swi3 sync tfr tsta tstb tst ldq stq lde ldf ldw ste stf stw adde addf addw tfm divd divq muld sexw"),
    directive=words("org equ set fcb fdb fcc fcs fqb rmb rmd zmb setdp end include incbin macro endm endc if ifdef ifndef else endif opt nam ttl spc page pragma section endsection export import align"),
    registers=words("a b d x y u s pc dp cc pcr e f w q v md"), numbers=hexnum, comments=(";", r"^\*"),
    label=r"^[A-Za-z_.@$][\w.@$]*:?(?=\s|$)")

grammar("assembly-8080", "Assembly 8080", ["8080", "8085", "i80", "asm8080", "asm8085"], """
Written for Limoniter from the Intel 8080 and 8085 instruction sets (Intel mnemonics, not Z80 ones), with the
directives of Intel's ASM and MAC and Microsoft's M80.""",
    "8080",
    branch=words("jmp jc jnc jz jnz jp jm jpe jpo call cc cnc cz cnz cp cm cpe cpo ret rc rnc rz rnz rp rm rpe rpo rst pchl hlt"),
    instruction=words("mov mvi lxi lda sta lhld shld ldax stax xchg add adi adc aci sub sui sbb sbi inr dcr inx dcx dad daa ana ani xra xri ora ori cmp cpi rlc rrc ral rar cma cmc stc push pop xthl sphl in out ei di nop rim sim"),
    directive=words("org equ set db dw ds end if else endif macro endm rept irp irpc local title name public extrn cseg dseg aseg page"),
    registers=words("a b c d e h l m sp psw"), numbers=r"(?i)(?<![\w.$@])[0-9][0-9a-f]*h(?![\w.])|(?<![\w.$@])[01]+b(?![\w.])|(?<![\w.$@])[0-7]+[oq](?![\w.])|(?<![\w.$@])\d+d?(?![\w.])|\$[0-9a-f]+")

grammar("assembly-65816", "Assembly 65816", ["65816", "816", "s816", "asm816", "a816"], """
Written for Limoniter from the WDC 65816 instruction set (with the 6502's and 65C02's it includes), as the SNES
and Apple IIGS use it, with the directives of ca65, WLA-DX, asar and Merlin 16, and .b/.w/.l size suffixes.""",
    "65816",
    branch=words("bcc bcs beq bmi bne bpl bra brl bvc bvs brk jmp jml jsr jsl rti rts rtl cop"),
    instruction=words("adc and asl bit clc cld cli clv cmp cpx cpy dec dex dey eor inc inx iny lda ldx ldy lsr nop ora pha php pla plp rol ror sbc sec sed sei sta stx sty tax tay tsx txa txs tya phx phy plx ply stz trb tsb mvn mvp pea pei per phb phd phk plb pld rep sep stp tcd tcs tdc tsc txy tyx wai wdm xba xce"),
    directive=words(".org .db .dw .dl .dd .byte .word .long .faraddr .addr .res .include .incbin .macro .endmacro .endm .if .else .endif .ifdef .ifndef .a8 .a16 .i8 .i16 .smart .segment .proc .endproc .export .import .define .bank .base .section .ends .memorymap .rombanks .equ org db dw dl dd incsrc incbin lorom hirom exlorom exhirom sa1 arch base macro endmacro if else endif equ dfb dfw ds mx"),
    registers=words("a x y s"), numbers=r"(?i)\$[0-9a-f]+|%[01]+|0x[0-9a-f]+|(?<![\w.$@])\d+(?![\w.])",
    word=r"[\w$@]", extra_main="    - match: '(?i)(?<![\\w$@])(?:{{instruction}}|{{branch}})\\.[bwl](?![\\w$@])'\n      scope: support.function.instruction.65816\n      push: operands")

cond = "eq|ne|cs|cc|mi|pl|vs|vc|hi|ls|ge|lt|gt|le|al|nv|hs|lo"
grammar("assembly-arm", "Assembly ARM (ARM2 and ARM3)", ["arm", "arm2", "arm3", "armasm", "s_arm"], """
Written for Limoniter from the Acorn ARM2 and ARM3 instruction sets, as RISC OS and the Archimedes use them, with
every condition code and the S, B, T and multiple-transfer suffixes, the BBC BASIC assembler's EQU and OPT
directives, objasm's, and GNU as's.""",
    "arm",
    branch=f"b(?:l)?(?:{cond})?|swi(?:{cond})?",
    instruction=(f"(?:adc|add|and|bic|eor|mov|mvn|orr|rsb|rsc|sbc|sub)(?:{cond})?s?|(?:cmn|cmp|teq|tst)(?:{cond})?p?|(?:mla|mul)(?:{cond})?s?"
                 f"|(?:ldr|str)(?:{cond})?b?t?|(?:ldm|stm)(?:{cond})?(?:ia|ib|da|db|fd|ed|fa|ea)|swp(?:{cond})?b?|(?:cdp|ldc|stc|mcr|mrc)(?:{cond})?l?|adr(?:{cond})?l?|nop"),
    directive=words("equb equw equd equs opt align dcb dcw dcd dcs dcfs dcfd area entry end export import global extern space fill macro mend mexit if else endif include get rn cn cp fn equ .text .data .bss .word .byte .short .long .ascii .asciz .align .global .globl .equ .set .include .macro .endm .if .endif .space .skip .ltorg ltorg"),
    registers=words("r0 r1 r2 r3 r4 r5 r6 r7 r8 r9 r10 r11 r12 r13 r14 r15 sp lr pc fp ip sl a1 a2 a3 a4 v1 v2 v3 v4 v5 v6 psr cpsr spsr"),
    numbers=r"(?i)&[0-9a-f]+|0x[0-9a-f]+|%[01]+|\b\d+\b", comments=(";", "//", "@ "), word=r"[\w$]")

grammar("assembly-pdp11", "Assembly PDP-11 (MACRO-11)", ["m11", "mac11", "pdp11", "p11"], """
Written for Limoniter from the DEC PDP-11 instruction set, with the EIS, FIS and FP11 instructions, and the
directives of MACRO-11. Numbers are octal unless they end in a full stop, and ^O, ^D, ^B, ^X and ^R say so.""",
    "pdp11",
    branch=words("br bne beq bpl bmi bvc bvs bcc bcs bge blt bgt ble bhi blos bhis blo jmp jsr rts rti rtt sob emt trap bpt iot mark"),
    instruction=words("clr clrb com comb inc incb dec decb neg negb tst tstb asr asrb asl aslb ror rorb rol rolb swab adc adcb sbc sbcb sxt mfps mtps mov movb cmp cmpb add sub bit bitb bic bicb bis bisb xor mul div ash ashc mfpi mtpi mfpd mtpd spl halt wait reset nop clc clv clz cln sec sev sez sen ccc scc fadd fsub fmul fdiv ldf stf addf subf mulf divf cmpf tstf clrf absf negf ldcif stcfi ldexp stexp modf ldfps stfps stst setf setd seti setl cfcc csm"),
    directive=words(".title .ident .psect .asect .csect .globl .word .byte .ascii .asciz .rad50 .blkb .blkw .even .odd .end .macro .endm .mexit .if .iff .ift .iftf .endc .iif .rept .irp .irpc .list .nlist .enabl .dsabl .mcall .include .library .page .sbttl .radix .limit .print .error .narg .nchr .ntype .flt2 .flt4 .packed .weak .cross .nocross .dsabl .enabl"),
    registers=words("r0 r1 r2 r3 r4 r5 r6 r7 sp pc ac0 ac1 ac2 ac3 ac4 ac5 %0 %1 %2 %3 %4 %5 %6 %7"),
    numbers=r"(?i)\^[odbx][0-9a-f]+|\^r[a-z0-9$. ]{1,3}|(?<![\w.$])\d+\.(?![\d\w])|(?<![\w.$])\d+(?![\w.])",
    label=r"^\s*[A-Za-z_.$][\w.$]*:{1,2}", bare_label=None, word=r"[\w.$]")

vax_types = "bwlqofdgh"
grammar("assembly-vax", "Assembly VAX MACRO", ["mar", "vaxmar", "vax"], """
Written for Limoniter from the DEC VAX instruction set, with its data type suffixes (B, W, L, Q, O, F, D, G, H)
and two- and three-operand forms, and the directives of VAX MACRO.""",
    "vax",
    branch=words("brb brw bneq bnequ beql beqlu bgtr bleq bgeq blss bgtru blequ bvc bvs bcc bcs blbs blbc bbs bbc bbss bbcs bbsc bbcc bbssi bbcci sobgtr sobgeq aoblss aobleq acbb acbw acbl acbf acbd acbg acbh jmp jsb rsb calls callg ret rei chmk chme chms chmu bpt halt caseb casew casel"),
    instruction=(f"(?:add|sub|mul|div|bis|bic|xor)[{vax_types}][23]|(?:mov|cmp|tst|clr|mcom|mneg|inc|dec|bit|push|mova|pusha)[{vax_types}]|movz(?:bw|bl|wl)|cvt[{vax_types}][{vax_types}]|cvtr[fdgh]l"
                 "|ashl|ashq|ashp|rotl|emul|ediv|extv|extzv|insv|ffs|ffc|cmpv|cmpzv|index|movc3|movc5|cmpc3|cmpc5|locc|skpc|scanc|spanc|matchc|movtc|movtuc|crc|editpc|addp4|addp6|subp4|subp6|mulp|divp|cmpp3|cmpp4|cvtlp|cvtpl|cvtps|cvtsp|cvttp|cvtpt|movp"
                 "|insque|remque|insqhi|insqti|remqhi|remqti|pushr|popr|movpsl|bispsw|bicpsw|prober|probew|mtpr|mfpr|ldpctx|svpctx|nop|adwc|sbwc|polyf|polyd|polyg|polyh|emodf|emodd|emodg|emodh"),
    directive=words(".title .ident .psect .entry .word .long .byte .quad .octa .ascii .ascic .ascid .asciz .blkb .blkw .blkl .blkq .blko .address .align .end .macro .endm .mexit .if .if_false .if_true .endc .iif .default .disable .enable .external .extrn .global .globl .list .nlist .show .noshow .sbttl .subtitle .transfer .mask .f_floating .d_floating .g_floating .h_floating .packed .repeat .rept .endr .irp .irpc .mcall .library .page .save_psect .restore_psect .link .weak"),
    registers=words("r0 r1 r2 r3 r4 r5 r6 r7 r8 r9 r10 r11 ap fp sp pc"),
    numbers=r"(?i)\^[odbx][0-9a-f]+|(?<![\w.$])\d+(?:\.\d*)?(?![\w.])",
    label=r"^\s*[A-Za-z_.$][\w.$]*:{1,2}", bare_label=None, word=r"[\w.$]")

grammar("assembly-avr", "Assembly AVR", ["avr", "avrasm", "s_avr"], """
Written for Limoniter from Atmel's (Microchip's) AVR 8-bit instruction set, with the directives of AVRASM2 and
GNU as. Comments are ;, // and /* */, as AVRASM2 takes.""",
    "avr",
    branch=words("brbc brbs brcc brcs break breq brge brhc brhs brid brie brlo brlt brmi brne brpl brsh brtc brts brvc brvs call cpse eicall eijmp icall ijmp jmp rcall ret reti rjmp sbic sbis sbrc sbrs"),
    instruction=words("adc add adiw and andi asr bclr bld bset bst cbi cbr clc clh cli cln clr cls clt clv clz com cp cpc cpi dec des elpm eor fmul fmuls fmulsu in inc lac las lat ld ldd ldi lds lpm lsl lsr mov movw mul muls mulsu neg nop or ori out pop push rol ror sbc sbci sbi sbiw sbr sec seh sei sen ser ses set sev sez sleep spm st std sts sub subi swap tst wdr xch"),
    directive=words(".byte .cseg .db .def .device .dseg .dw .dd .dq .endm .endmacro .equ .eseg .exit .include .list .listmac .macro .nolist .org .set .if .else .elif .endif .error .warning .message .ifdef .ifndef .undef .overlap .nooverlap .section .text .data .global .word .ascii .asciz .space .align"),
    registers=words("r0 r1 r2 r3 r4 r5 r6 r7 r8 r9 r10 r11 r12 r13 r14 r15 r16 r17 r18 r19 r20 r21 r22 r23 r24 r25 r26 r27 r28 r29 r30 r31 x y z xl xh yl yh zl zh sreg sp spl sph"),
    numbers=r"(?i)0x[0-9a-f]+|\$[0-9a-f]+|0b[01]+|(?<![\w.$@])\d+(?![\w.])", comments=(";", "//", "/*"))

grammar("assembly-pic", "Assembly PIC (MPASM)", ["pic", "mpasm", "pic16", "pic18"], """
Written for Limoniter from Microchip's PIC mid-range (PIC16) and PIC18 instruction sets, with MPASM's directives.
The skip instructions (DECFSZ, BTFSS and the like) are coloured as branches, since they decide what runs next.""",
    "pic",
    branch=words("goto call return retlw retfie bra rcall bz bnz bc bnc bn bnn bov bnov decfsz incfsz dcfsnz infsnz btfsc btfss cpfseq cpfsgt cpfslt tstfsz reset"),
    instruction=words("addwf andwf clrf clrw comf decf incf iorwf movf movwf nop rlf rrf subwf swapf xorwf bcf bsf addlw andlw clrwdt iorlw movlw sleep sublw xorlw movff lfsr addwfc mullw mulwf negf rlcf rlncf rrcf rrncf setf subfwb subwfb btg movlb tblrd tblwt push pop daw option tris"),
    directive=words("__config __idlocs __badram __maxram list nolist include #include processor radix org equ set cblock endc banksel pagesel bankisel dt dw db de data res end macro endm local exitm if else endif ifdef ifndef #define #undefine #if #else #endif errorlevel constant variable global extern udata udata_shr idata code config title subtitle messg error noexpand expand space page fill"),
    registers=words("w f a access banked status pcl pclath fsr indf intcon wreg bsr tosu tosh tosl fsr0 fsr1 fsr2 indf0 indf1 indf2"),
    numbers=r"(?i)[hdbo]'[0-9a-f]+'|0x[0-9a-f]+|(?<![\w.$@])[0-9][0-9a-f]*h(?![\w.])|\.\d+|(?<![\w.$@])\d+(?![\w.])", strings=('"',),
    label=r"^[A-Za-z_][\w]*:?(?=\s|$)", word=r"[\w.$@#]")
