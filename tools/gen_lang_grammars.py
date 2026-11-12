#!/usr/bin/env python3
"""Writes Limoniter's grammars for older and retro languages into data/syntaxes/, from one layout.
Usage: python3 -I tools/gen_lang_grammars.py data/syntaxes

Each language's keywords, comments, strings and numbers are from its published definition, written from memory
rather than checked line by line (see MECHANICS.md). Keywords are matched whole, by the language's own idea of
what a word is, and case-insensitively where the language is.
"""
import os
import re
import sys

OUT = sys.argv[1]


def q(pattern):
    """A pattern as a single-quoted YAML string."""
    return "'" + pattern.replace("'", "''") + "'"


def alternation(words):
    return "|".join(re.escape(w) for w in sorted(set(words.split()), key=lambda w: (-len(w), w)))


def grammar(key, name, exts, scope, header, *, groups, ci=True, word=r"[A-Za-z0-9_]", before=None, after=None,
            comments=(), strings=(), numbers=r"(?<![\w.])\d+(?:\.\d+)?(?:[eE][+-]?\d+)?(?![\w.])", operators=None,
            first_line=None, rules=(), extra_contexts=""):
    before = before or "(?<!" + word + ")"
    after = after or "(?!" + word + ")"
    flag = "(?i)" if ci else ""
    lines = ["%YAML 1.2", "---"] + ["# " + l for l in header.strip().splitlines()]
    lines += [f"name: {name}", "file_extensions: [" + ", ".join('"' + e + '"' for e in exts) + "]"]
    if first_line:
        lines.append(f"first_line_match: {q(first_line)}")
    lines += [f"scope: source.{scope}", "contexts:", "  main:"]
    for i, comment in enumerate(comments):
        if "line" in comment:
            lines += [f"    - match: {q(comment['line'])}", f"      scope: comment.line.{scope}"]
        else:
            lines += [f"    - match: {q(comment['begin'])}", f"      push: comment_{i}"]
    for i, string in enumerate(strings):
        lines += [f"    - match: {q(string['begin'])}", f"      push: string_{i}"]
    for rule in rules:
        regex, what = rule
        lines.append(f"    - match: {q(regex)}")
        if isinstance(what, dict):
            lines.append("      captures:")
            for n, s in what.items():
                lines.append(f"        {n}: {s}.{scope}")
        else:
            lines.append(f"      scope: {what}.{scope}")
    for kind, words in groups:
        lines += [f"    - match: {q(flag + before + '(?:' + alternation(words) + ')' + after)}", f"      scope: {kind}.{scope}"]
    if numbers:
        lines += [f"    - match: {q(numbers)}", f"      scope: constant.numeric.{scope}"]
    if operators:
        lines += [f"    - match: {q(operators)}", f"      scope: keyword.operator.{scope}"]
    for i, comment in enumerate(comments):
        if "line" in comment:
            continue
        lines += [f"  comment_{i}:", f"    - meta_scope: comment.block.{scope}"]
        if comment.get("nest"):
            lines += [f"    - match: {q(comment['begin'])}", f"      push: comment_{i}"]
        lines += [f"    - match: {q(comment['end'])}", "      pop: true"]
    for i, string in enumerate(strings):
        lines += [f"  string_{i}:", f"    - meta_scope: string.quoted.{scope}"]
        if string.get("escape"):
            lines += [f"    - match: {q(string['escape'])}", f"      scope: constant.character.escape.{scope}"]
        if string.get("nest"):
            lines += [f"    - match: {q(string['begin'])}", f"      push: string_{i}"]
        lines += [f"    - match: {q(string['end'])}", "      pop: true"]
        if not string.get("multiline"):
            lines += ["    - match: '(?=\\n)'", "      pop: true"]
    open(os.path.join(OUT, key + ".sublime-syntax"), "w", newline="\n").write("\n".join(lines) + "\n" + extra_contexts)


dq = {"begin": '"', "end": '"', "escape": '""'}
sq = {"begin": "'", "end": "'", "escape": "''"}
basic_string = {"begin": '"', "end": '"'}

grammar("forth", "Forth", ["fth", "4th", "forth", "f83", "fb", "frt"], "forth", """
Forth as ANS Forth (1994) and Forth-2012 define it, with the common words of FIG-Forth and Forth-83. Words are
anything between spaces, so a keyword is only one when it stands alone. A definition's name follows its colon.""",
    word=r"\S", before=r"(?<!\S)", after=r"(?!\S)",
    comments=[{"line": r"(?<!\S)\\(?:\s.*)?$"}, {"begin": r"(?<!\S)\((?=\s)", "end": r"\)"}],
    strings=[{"begin": r'(?i)(?<!\S)(?:\.|s|c|abort|\,)"(?=\s)', "end": '"'}, {"begin": r"(?<!\S)\.\((?=\s)", "end": r"\)"}],
    rules=[(r"(?<!\S)(:|:noname)\s+(\S+)", {1: "keyword.declaration", 2: "entity.name.function"}),
           (r"(?i)(?<!\S)(variable|constant|value|create|2variable|2constant|fvariable|fconstant|defer|buffer:|marker)\s+(\S+)", {1: "keyword.declaration", 2: "entity.name.constant"})],
    groups=[("keyword.control", "if else then begin until while repeat again do ?do loop +loop leave unloop exit recurse case of endof endcase ; ;code does> immediate postpone [ ] literal [char] ['] '"),
            ("support.function", "dup drop swap over rot -rot nip tuck pick roll ?dup 2dup 2drop 2swap 2over >r r> r@ 2>r 2r> @ ! +! c@ c! 2@ 2! . u. .r emit cr type space spaces key accept here allot , c, cells cell+ chars char+ align aligned and or xor invert negate abs min max mod /mod */ */mod = <> < > u< u> 0= 0<> 0< 0> 1+ 1- 2* 2/ lshift rshift true false base decimal hex bl count move fill erase cmove cmove> execute evaluate find word parse source >in state char depth bye words see forget include included fetch store")],
    numbers=r"(?i)(?<!\S)[-+]?(?:\$[0-9a-f]+|%[01]+|#\d+|\d+(?:\.\d*)?(?:e[-+]?\d+)?)(?!\S)")

grammar("modula2", "Modula-2", ["mod", "def", "mi", "m2"], "modula2", """
Modula-2 as Niklaus Wirth's "Programming in Modula-2" (fourth edition) defines it, with ISO Modula-2's additions.
Keywords are capitals, comments (* *) nest, and a procedure's or module's name follows its keyword.""",
    ci=False, comments=[{"begin": r"\(\*", "end": r"\*\)", "nest": True}],
    strings=[{"begin": '"', "end": '"'}, {"begin": "'", "end": "'"}],
    rules=[(r"\b(PROCEDURE|MODULE)\s+([A-Za-z_]\w*)", {1: "keyword.declaration", 2: "entity.name.function"})],
    groups=[("keyword.control", "AND ARRAY BEGIN BY CASE CONST DEFINITION DIV DO ELSE ELSIF END EXCEPT EXIT EXPORT FINALLY FOR FORWARD FROM IF IMPLEMENTATION IMPORT IN LOOP MOD MODULE NOT OF OR PACKEDSET POINTER PROCEDURE QUALIFIED RECORD REM REPEAT RETRY RETURN SET THEN TO TYPE UNTIL VAR WHILE WITH"),
            ("storage.type", "BOOLEAN CARDINAL CHAR INTEGER LONGINT LONGCARD LONGREAL REAL BITSET PROC COMPLEX LONGCOMPLEX ADDRESS WORD BYTE"),
            ("constant.language", "TRUE FALSE NIL"),
            ("support.function", "ABS CAP CHR DEC EXCL FLOAT HALT HIGH INC INCL MAX MIN ODD ORD SIZE TRUNC VAL NEW DISPOSE LENGTH TSIZE ADR")],
    numbers=r"(?<![\w.])(?:[0-9][0-9A-F]*H|[0-7]+[BC]|\d+(?:\.\d*)?(?:E[+-]?\d+)?)(?![\w.])", operators=r":=|<>|<=|>=|[#&~+\-*/=<>^]")

grammar("smalltalk", "Smalltalk", ["st", "sources", "changes"], "smalltalk", """
Smalltalk-80 as the Blue Book describes it, as Squeak, Pharo and VisualWorks write it: "comments", 'strings',
#symbols, $characters, keyword selectors ending in a colon, block arguments, := and the return ^.""",
    ci=False, comments=[{"begin": '"', "end": '"'}], strings=[{"begin": "'", "end": "'", "escape": "''", "multiline": True}],
    rules=[(r"#(?:[A-Za-z_][\w:]*|'[^']*'|[-+*/\\<>=~@%|&?!,]+)", "constant.other.symbol"),
           (r"\$.", "constant.character"),
           (r":[A-Za-z_]\w*", "variable.parameter"),
           (r"\b[A-Za-z_]\w*:(?!=)", "entity.name.function"),
           (r"\^|:=|←", "keyword.control")],
    groups=[("variable.language", "self super thisContext"), ("constant.language", "true false nil")],
    numbers=r"(?<![\w.])-?(?:\d+r[0-9A-Z]+|\d+(?:\.\d+)?(?:e-?\d+)?)(?![\w])")

grammar("prolog", "Prolog", ["pro", "prolog", "plg"], "prolog", """
Prolog as the ISO standard (ISO/IEC 13211-1) and Edinburgh Prolog have it: % and /* */ comments, 'atoms' and
"strings", variables that start with a capital or _, the neck :- and the common built-in predicates.""",
    ci=False, comments=[{"line": r"%.*$"}, {"begin": r"/\*", "end": r"\*/"}],
    strings=[{"begin": '"', "end": '"', "escape": r'\\.|""'}, {"begin": "'", "end": "'", "escape": r"\\.|''"}],
    rules=[(r"^[a-z]\w*(?=\s*(?:\(|:-))", "entity.name.function"), (r"\b(?:[A-Z]\w*|_\w*)", "variable.other"), (r":-|\?-|-->", "keyword.control")],
    groups=[("keyword.control", "is mod rem not fail true false repeat call findall bagof setof forall catch throw once halt"),
            ("support.function", "assert asserta assertz retract abolish write writeln print nl read consult atom number integer float var nonvar compound callable atomic is_list functor arg copy_term atom_codes atom_chars atom_length char_code number_codes sub_atom append member length reverse nth0 nth1 msort sort keysort format between succ plus")],
    numbers=r"(?<![\w.])\d+(?:\.\d+)?(?:[eE][+-]?\d+)?|0'.", operators=r"\\\+|=\.\.|==|\\==|=:=|=\\=|=<|>=|@<|@>|[!;,|=<>+\-*/]")

grammar("apl", "APL", ["apl", "apln", "aplf", "aplc", "dyalog", "dws"], "apl", """
APL as Dyalog and ISO APL (ISO 8485) write it: the ⍝ comment, 'strings', ¯ for negative numbers, the primitive
functions and operators by their symbols, system names starting with ⎕, and Dyalog's :If-style control words.""",
    ci=True, word=r"[\w∆⍙]", comments=[{"line": r"⍝.*$"}], strings=[{"begin": "'", "end": "'", "escape": "''"}],
    rules=[(r"⎕\w*|⍞", "support.function"), (r"[⍺⍵∇⍬]", "variable.language"), (r"[←→]", "keyword.control"),
           (r"(?i)(?<![\w])(?::(?:If|ElseIf|Else|EndIf|AndIf|OrIf|For|In|InEach|EndFor|While|EndWhile|Repeat|Until|EndRepeat|Return|Leave|Continue|Select|Case|CaseList|EndSelect|Trap|EndTrap|With|EndWith|Hold|EndHold|Namespace|EndNamespace|Class|EndClass|Field|Property|EndProperty|Access|Implements|Include|GoTo|End))(?![\w])", "keyword.control")],
    groups=[],
    numbers=r"¯?\d+(?:\.\d+)?(?:[eE]¯?\d+)?(?:[jJ]¯?\d+(?:\.\d+)?)?",
    operators=r"[+\-×÷⌈⌊|⍳⍴⌽⊖⍉∊⍷⍋⍒~∧∨⍲⍱<≤=≥>≠≡≢⊂⊃⊆⊇∪∩⊥⊤⍕⍎○!?⍟⌹⊣⊢¨⍨⍣∘./\\⌿⍀⍤⍥@⌸⌺⍠,⍪↑↓⋄]")

grammar("mumps", "MUMPS (M)", ["mumps", "mps", "ro", "rsa", "zwr"], "mumps", """
MUMPS (M) as ANSI/MDC X11.1 defines it: commands and their one-letter abbreviations at the start of a statement,
$ intrinsic functions and special variables, ^ globals, labels in the first column and ; comments.""",
    comments=[{"line": r";.*$"}], strings=[{"begin": '"', "end": '"', "escape": '""'}],
    rules=[(r"^[%A-Za-z0-9][A-Za-z0-9]*(?=[\s(;]|$)", "entity.name.function"),
           (r"\^[%A-Za-z][A-Za-z0-9]*", "variable.other.global"),
           (r"(?i)\$(?:ASCII|A|CHAR|C|DATA|D|EXTRACT|E|FIND|F|FNUMBER|FN|GET|G|JUSTIFY|J|LENGTH|L|NAME|NA|ORDER|O|PIECE|P|QLENGTH|QL|QSUBSCRIPT|QS|QUERY|Q|RANDOM|R|REVERSE|RE|SELECT|S|STACK|ST|TEXT|T|TRANSLATE|TR|VIEW|V|Z\w*)(?=\()", "support.function"),
           (r"(?i)\$(?:DEVICE|ECODE|EC|ESTACK|ES|ETRAP|ET|HOROLOG|H|IO|I|JOB|J|KEY|K|PRINCIPAL|PR|QUIT|Q|STACK|ST|STORAGE|S|SYSTEM|SY|TEST|T|TLEVEL|TL|X|Y|Z\w*)\b", "variable.language"),
           (r"\$\$[%A-Za-z]\w*", "entity.name.function")],
    before=r"(?:(?<=^[ \t])|(?<=[ \t:.])|(?<=^))", after=r"(?=[ :]|$)",
    groups=[("keyword.control", "BREAK B CLOSE C DO D ELSE E FOR F GOTO G HALT H HANG IF I JOB J KILL K LOCK L MERGE M NEW N OPEN O QUIT Q READ R SET S TCOMMIT TC TRESTART TRE TROLLBACK TRO TSTART TS USE U VIEW V WRITE W XECUTE X")],
    numbers=r"(?<![\w.])\d+(?:\.\d+)?(?:E[+-]?\d+)?", operators=r"'?[=<>\[\]]|[+\-*/\\#_&!'?@]")

grammar("xbase", "xBase (dBase, Clipper, FoxPro)", ["prg", "ch", "clp", "xbase", "dbs", "fmt", "qpr"], "xbase", """
The xBase family, dBase III and IV, Clipper and FoxPro: * comments at the start of a line, && and // after code,
NOTE, '' "" and [] strings, the commands and their four-letter abbreviations, and the functions.""",
    comments=[{"line": r"^\s*(?:\*|(?i:note\b)).*$"}, {"line": r"&&.*$|//.*$"}, {"begin": r"/\*", "end": r"\*/"}],
    strings=[{"begin": '"', "end": '"'}, {"begin": "'", "end": "'"}],
    rules=[(r"(?i)\b(procedure|function|proc|func|method|class)\s+([A-Za-z_]\w*)", {1: "keyword.declaration", 2: "entity.name.function"}),
           (r"(?i)\.(?:t|f|y|n|and|or|not)\.", "constant.language"),
           (r"(?i)\b[A-Za-z_]\w*(?=\()", "support.function"),
           (r"^\s*#\s*(?:include|define|command|translate|xcommand|xtranslate|ifdef|ifndef|else|endif|undef)\b", "keyword.other.preprocessor")],
    groups=[("keyword.control", "use sele select go goto skip seek find loca locate cont continue repl replace appe append blank dele delete reca recall pack zap inde index on reindex set to close all databases stor store do while enddo if else elseif endif case othe otherwise endc endcase for next exit loop retu return proc procedure func function para parameters priv private publ public local static memvar field decl declare say get read clea clear acce accept input wait quit canc cancel run copy sort tota total coun count sum aver average disp display list brow browse edit crea create modi modify stru structure repo report labe label form scan endscan with sequence endsequence begin end class method data inherit from create local default"),
            ("constant.language", "nil")],
    numbers=r"(?<![\w.])\d+(?:\.\d+)?(?![\w])")

grammar("logo", "Logo", ["lgo", "logo", "lg"], "logo", """
Logo as UCBLogo and the classic turtle Logos have it: TO ... END procedures, "quoted words, :variables, [lists],
; comments, the turtle commands and their short forms, and the list primitives.""",
    word=r"[\w.?]", comments=[{"line": r";.*$"}],
    rules=[(r"(?i)^\s*(to)\s+([^\s\[\]()]+)", {1: "keyword.declaration", 2: "entity.name.function"}),
           (r":[^\s\[\]()]+", "variable.other"), (r"\"[^\s\[\]()]*", "string.unquoted")],
    groups=[("keyword.control", "to end make name local localmake output op stop repeat if ifelse iftrue iffalse test forever while until for do.while do.until run runresult catch throw wait"),
            ("support.function", "forward fd back bk left lt right rt penup pu pendown pd penerase pe home clearscreen cs clean setxy setpos setx sety setheading seth hideturtle ht showturtle st print pr show type setpencolor setpc setbackground setbg setpensize arc circle random sum difference product quotient remainder first last butfirst bf butlast bl item fput lput list sentence se word count emptyp memberp listp wordp numberp equalp sqrt int round label fill towards heading pos xcor ycor")],
    numbers=r"(?<![\w.])-?\d+(?:\.\d+)?")

grammar("pli", "PL/I", ["pli", "pl1", "plx", "pli1"], "pli", """
PL/I as IBM's Enterprise PL/I and the ANSI X3.53 standard have it: /* */ comments, strings with 'B and 'X
suffixes, labels before a colon, the statements, the attributes and the commonest built-in functions.""",
    comments=[{"begin": r"/\*", "end": r"\*/"}], strings=[{"begin": "'", "end": r"'(?!')[BXbx]?", "escape": "''"}, {"begin": '"', "end": '"(?!")', "escape": '""'}],
    rules=[(r"(?i)^\s*([A-Za-z_$#@][\w$#@]*)\s*(:)\s*(proc(?:edure)?|package)\b", {1: "entity.name.function", 3: "keyword.declaration"}),
           (r"^\s*([A-Za-z_$#@][\w$#@]*)\s*(:)(?!=)", {1: "entity.name.label"}),
           (r"^\s*%\w+", "keyword.other.preprocessor")],
    groups=[("keyword.control", "procedure proc package end do if then else declare dcl call return go to goto on begin select when otherwise other leave iterate while until repeat by allocate alloc free open close read write rewrite delete locate get put list edit data skip page line file into from key keyto set signal revert stop exit format entry fetch release display reply wait halt"),
            ("storage.type", "fixed float binary bin decimal dec character char bit varying var varyingz nonvarying pointer ptr picture pic complex real label area offset aligned unaligned init initial static automatic auto based controlled ctl defined def external ext internal int options main recursive returns builtin like dimension dim condition generic handle ordinal type union structure value byaddr byvalue"),
            ("support.function", "substr length index verify translate addr null sysnull high low mod abs max min sum prod round trunc ceil floor sign date time datetime oncode onloc onchar substr repeat copy trim left right center hex unspec string allocation hbound lbound dim lowercase uppercase search")],
    numbers=r"(?<![\w.])\d+(?:\.\d+)?(?:[eE][+-]?\d+)?[bBiI]?")

grammar("rpg", "RPG (IBM i)", ["rpg", "rpgle", "sqlrpgle", "rpg38", "rpgile", "rpgleinc"], "rpg", """
IBM's RPG III and RPG IV (ILE RPG), fixed form and free form: a comment line has * in column 7, or //; column 6
holds the specification type (H, F, D, I, C, O, P); and the operation codes, declarations, %built-in functions,
*figurative constants and *INxx indicators are coloured wherever they are.""",
    comments=[{"line": r"^.{5}[*].*$|^.{6}\*.*$|//.*$"}], strings=[{"begin": "'", "end": "'", "escape": "''"}],
    rules=[(r"^(.{5})([HFDICOPEL])(?=.)", {1: "comment.line.sequence", 2: "keyword.other.specification"}),
           (r"^\s*\*\*\s*free\b.*$", "keyword.other.directive"),
           (r"(?i)%[a-z]+(?=\s*\(|\b)", "support.function"),
           (r"(?i)\*in(?:\d\d|lr|rt|h\d|l\d|[a-z]{2})\b", "variable.language"),
           (r"(?i)\*(?:on|off|blanks?|zeros?|hival|loval|null|omit|all|nopass|varsize|start|end|entry|inzsr|pssr|date|time|iso|usa|eur|jis)\b", "constant.language"),
           (r"(?i)\b(dcl-proc|begsr)\s+([A-Za-z_][\w#@$]*)", {1: "keyword.declaration", 2: "entity.name.function"})],
    groups=[("keyword.control", "if else elseif endif dow dou enddo for endfor select when other endsl iter leave return eval evalr callp call callb exsr begsr endsr monitor on-error endmon read reade readp readpe chain setll setgt write update delete open close exfmt dsply clear reset sorta lookup xfoot movel move z-add z-sub add sub mult div mvr comp cabeq goto tag cas endcs parm plist klist kfld dcl-s dcl-c dcl-ds end-ds dcl-f dcl-pr end-pr dcl-pi end-pi dcl-proc end-proc dcl-subf dcl-parm ctl-opt exec sql"),
            ("storage.type", "char varchar int uns packed zoned bindec float date time timestamp ind pointer like likeds likerec dim inz const value options extpgm extproc qualified template static based export import")],
    numbers=r"(?<![\w.])\d+(?:\.\d+)?")

grammar("basic-qbasic", "BASIC (QBasic, QuickBASIC and GW-BASIC)", ["bi", "bm", "qb", "qbs", "gwbas"], "basic.qbasic", """
Microsoft's BASICs for the PC: GW-BASIC and BASICA with their line numbers, and QBasic and QuickBASIC with labels,
SUB and FUNCTION, types and AS. ' and REM comments, $ % ! # & type suffixes, &H and &O numbers, and the
statements and functions of QuickBASIC 4.5.""",
    comments=[{"line": r"'.*$"}, {"line": r"(?i)(?<![\w$])rem(?![\w$]).*$"}], strings=[{"begin": '"', "end": '"'}],
    rules=[(r"^\s*\d+", "constant.numeric.line-number"),
           (r"(?i)\b(sub|function|def\s+fn)\s+([A-Za-z_][\w.]*[$%!#&]?)", {1: "keyword.declaration", 2: "entity.name.function"}),
           (r"^\s*[A-Za-z_][\w.]*:(?!=)", "entity.name.label"),
           (r"(?i)&h[0-9a-f]+&?|&o?[0-7]+&?", "constant.numeric")],
    word=r"[\w$%!#&.]",
    groups=[("keyword.control", "if then else elseif end select case is do loop while wend until for to step next exit goto gosub return on error resume sub function declare call def fn shared static dim redim common const type as let print lprint input line write open close get put seek field lset rset chain run stop system cls locate color screen width view window pset preset circle draw paint palette pcopy beep sound play randomize swap erase data read restore option base kill name chdir mkdir rmdir files shell key timer clear lock unlock using out poke bload bsave"),
            ("storage.type", "integer long single double string any"),
            ("support.function", "abs asc atn cdbl chr$ cint clng cos csng csrlin cvd cvi cvl cvs date$ environ$ eof erl err exp fix fre freefile hex$ inkey$ inp input$ instr int lbound lcase$ left$ len loc lof log ltrim$ mid$ mkd$ mki$ mkl$ mks$ oct$ peek pmap point pos right$ rnd rtrim$ sgn sin space$ spc sqr stick str$ strig string$ tab tan time$ ubound ucase$ val varptr varseg")],
    numbers=r"(?<![\w.$])\d+(?:\.\d+)?(?:[eEdD][+-]?\d+)?[%!#&]?")

grammar("basic-amos", "BASIC (AMOS)", ["amos", "amosasc"], "basic.amos", """
AMOS and AMOS Professional, the Amiga's games BASIC, as listings saved as text: ' and Rem comments, Procedure and
End Proc, and its screen, bob, sprite, AMAL and sound commands, many of them two words long.""",
    comments=[{"line": r"'.*$"}, {"line": r"(?i)(?<!\w)rem(?!\w).*$"}], strings=[{"begin": '"', "end": '"'}],
    rules=[(r"(?i)\b(procedure)\s+([A-Za-z_]\w*)", {1: "keyword.declaration", 2: "entity.name.function"}),
           (r"(?i)\b(?:end proc|end if|wait vbl|wait key|screen open|screen close|screen display|screen offset|screen to front|double buffer|paste bob|get bob|bob off|sprite off|load iff|save iff|get palette|set rainbow|rainbow del|amal on|amal off|channel to|track loop on|track play|sam play|key state|mouse key|x mouse|y mouse|hide on|show on|curs off|flash off|set text|text style|set font|get fonts)\b", "keyword.control"),
           (r"^\s*[A-Za-z_]\w*:", "entity.name.label")],
    word=r"[\w$#]",
    groups=[("keyword.control", "if then else end endif for to step next repeat until do loop while wend goto gosub return on proc procedure param shared global dim let print input exit exit if break off every pop"),
            ("support.function", "screen bob sprite ink bar box circle ellipse plot draw polygon paint colour palette cls home locate pen paper text centre amal amreg anim synchro boom shoot bell sam music tempo volume voice play wait timer rnd randomize abs int sgn sqr sin cos tan len left$ right$ mid$ chr$ asc str$ val instr upper$ lower$ string$ space$ joy fire jleft jright jup jdown inkey$ scancode zone reserve erase bload bsave start length")],
    numbers=r"(?<![\w.$#])(?:\$[0-9A-Fa-f]+|%[01]+|\d+(?:\.\d+)?)")

grammar("basic-blitz", "BASIC (Blitz Basic)", ["bb", "bb2", "blitz", "bbs"], "basic.blitz", """
Blitz Basic 2 on the Amiga (and the Blitz3D and BlitzPlus it led to): ; comments, Statement and Function with their
types (.w .l .q .s .b), NEWTYPE, Blitz mode commands and its display, shape and slice commands.""",
    comments=[{"line": r";.*$"}], strings=[{"begin": '"', "end": '"'}],
    rules=[(r"(?i)\b(statement|function|newtype|type)\s*(?:\.\w+\s+)?([A-Za-z_]\w*)", {1: "keyword.declaration", 2: "entity.name.function"}),
           (r"\.[wlqsbf](?!\w)", "storage.type"), (r"^\s*\.[A-Za-z_]\w*", "entity.name.label")],
    groups=[("keyword.control", "if then else elseif endif end select case default for to step next repeat until forever while wend goto gosub return statement function newtype field dim shared global local const include xinclude blitz amiga qamiga vwait exit endstatement endfunction"),
            ("support.function", "bitmap shape slice show showf display initcopper coplist bitplanesbitmap cls print nprint locate colour rgb palette loadshape loadshapes loadpalette blit qblit bblit scroll buffer mouse joyx joyy joyb inkey$ rnd int abs sqr sin cos len left$ right$ mid$ chr$ asc str$ val instr")],
    numbers=r"(?<![\w.$])(?:\$[0-9A-Fa-f]+|%[01]+|\d+(?:\.\d+)?)")

grammar("basic-stos", "BASIC (STOS)", ["stos", "stosasc"], "basic.stos", """
STOS Basic, the Atari ST's games BASIC, as listings saved as text: line numbers, rem and ' comments, and its
sprite, screen, music and zone commands.""",
    comments=[{"line": r"'.*$"}, {"line": r"(?i)(?<!\w)rem(?!\w).*$"}], strings=[{"begin": '"', "end": '"'}],
    rules=[(r"^\s*\d+", "constant.numeric.line-number")],
    groups=[("keyword.control", "if then else end for to step next repeat until while wend goto gosub return on dim let print input stop wait"),
            ("support.function", "sprite move anim limit sprite update reduce screen$ screen copy logic physic back appear fade flash shift colour palette ink pen paper bar box circle ellipse plot draw polygon paint cls home locate centre music voice volume boom shoot bell zone set zone reset zone mouse x mouse y mouse key hide show curs off key off erase reserve load save rnd randomize abs int sgn sqr sin cos len left$ right$ mid$ chr$ asc str$ val instr upper$ lower$ string$ space$")],
    numbers=r"(?<![\w.$])(?:\$[0-9A-Fa-f]+|%[01]+|\d+(?:\.\d+)?)")

grammar("amigaguide", "AmigaGuide", ["guide", "amigaguide"], "amigaguide", """
AmigaGuide hypertext, as the Amiga's AmigaGuide and MultiView read it: @ commands at the start of a line (@DATABASE,
@NODE, @ENDNODE and the rest), @{" link " LINK node} buttons, @{b} style commands in the text, and \\ escapes.""",
    first_line=r"(?i)^@database\b",
    rules=[(r"(?i)^(@node)\s+(\S+|\"[^\"]*\")", {1: "keyword.control", 2: "entity.name.section"}),
           (r"(?i)^@(?:remark|rem)\b.*$", "comment.line"),
           (r"(?i)^@\w+", "keyword.control"),
           (r"(?i)(@\{)(\"[^\"]*\")\s*(link|alink|rx|rxs|system|beep|close|quit)\b([^}]*)(\})", {1: "punctuation.definition.tag", 2: "string.quoted", 3: "keyword.other", 4: "entity.name.tag", 5: "punctuation.definition.tag"}),
           (r"@\{[^}]*\}", "keyword.other"), (r"\\[\\@]", "constant.character.escape")],
    groups=[], numbers=None)

grammar("amiga-installer", "Amiga Installer", ["installer", "install", "Install"], "installer", """
Amiga Installer scripts, the Lisp-like language of Commodore's Installer: ; comments, "strings" with \\ escapes,
(statements) and their (parameters), and the Installer's variables, such as @default-dest.""",
    comments=[{"line": r";.*$"}], strings=[{"begin": '"', "end": '"', "escape": r"\\."}, {"begin": "'", "end": "'"}],
    rules=[(r"@[\w-]+", "variable.language"), (r"(?<=\()\s*(procedure)\s+([\w-]+)", {1: "keyword.declaration", 2: "entity.name.function"})],
    word=r"[\w-]", before=r"(?<=\()\s*",
    groups=[("keyword.control", "set procedure if while until select trace retrace welcome message working askdir askfile askstring asknumber askchoice askoptions askbool copyfiles copylib startup tooltype textfile execute run rexx makedir makeassign delete rename protect exit abort onerror trap complete user debug cat tackon fileonly pathonly expandpath getenv getsize getversion getsum getdevice getdiskspace getassign exists earlier database patmatch strlen substr transcript foreach iconinfo closemedia effect setmedia showmedia symbolset symbolval openwbobject showwbobject closewbobject"),
            ("support.function", "prompt help source dest pattern choices default all newname confirm safe optional nogauge files infos noposition fonts append include command range disk assigns resident override delopts settooltype setdefaulttool setstack noicon swapcolors")],
    numbers=r"(?<![\w.$])(?:\$[0-9A-Fa-f]+|%[01]+|-?\d+)")

grammar("config-sys", "CONFIG.SYS (DOS)", ["config.sys", "CONFIG.SYS", "sys"], "configsys", """
MS-DOS's and PC DOS's CONFIG.SYS: REM and ; comments, the directives before their = (DEVICE, FILES, BUFFERS,
DOS and the rest), the [menu] blocks of MS-DOS 6 and the device driver paths after them.""",
    comments=[{"line": r"(?i)^\s*(?:rem\b|;).*$"}],
    rules=[(r"^\s*\[[^\]]*\]", "entity.name.section"),
           (r"(?i)^\s*(device|devicehigh|install|installhigh|shell)(\s*=\s*)(\S+)", {1: "keyword.control", 3: "string.unquoted"}),
           (r"(?i)^\s*(files|buffers|dos|stacks|lastdrive|country|break|fcbs|drivparm|numlock|switches|set|include|menuitem|menudefault|menucolor|submenu|multitrack|lh|loadhigh|verify|cpu|dosdata)(?=\s*=|\s)", "keyword.control")],
    groups=[("constant.language", "on off high low umb noumb auto noauto")], numbers=r"(?<![\w.])\d+(?![\w.])")

grammar("rtf", "RTF", ["rtf"], "rtf", """
Rich Text Format as Microsoft's RTF specification (1.9.1) describes it: { } groups, \\control words with their
numbers, \\' hexadecimal characters, \\* ignorable destinations and the font, colour and style tables.""",
    first_line=r"^\{\\rtf", ci=False,
    rules=[(r"\\'[0-9a-fA-F]{2}", "constant.character.escape"),
           (r"\\\*", "keyword.other"),
           (r"(\\(?:fonttbl|colortbl|stylesheet|info|pict|object|field|fldinst|fldrslt|header|footer|footnote))(?![a-zA-Z])", {1: "entity.name.section"}),
           (r"\\[a-zA-Z]+-?\d*", "keyword.control"),
           (r"\\[^a-zA-Z]", "constant.character.escape"),
           (r"[{}]", "punctuation.section.group")],
    groups=[], numbers=None)

grammar("postscript", "PostScript", ["ps", "eps", "epsf", "epsi", "pfa"], "postscript", """
PostScript as Adobe's PostScript Language Reference (third edition) has it: % comments with %% DSC comments kept
apart, (strings) that nest with \\ escapes, <hex> and <~ascii85~> strings, /literal names, { } procedures,
radix numbers such as 16#FF, and the operators of the language.""",
    first_line=r"^%!", ci=False, word=r"[^\s()<>\[\]{}/%]", before=r"(?<![^\s()<>\[\]{}/%])", after=r"(?![^\s()<>\[\]{}/%])",
    comments=[{"line": r"^%%.*$|^%!.*$"}, {"line": r"%.*$"}],
    strings=[{"begin": r"\(", "end": r"\)", "escape": r"\\(?:[0-7]{1,3}|.)", "nest": True, "multiline": True}, {"begin": r"<~", "end": r"~>", "multiline": True}],
    rules=[(r"<[0-9A-Fa-f\s]*>", "string.other.hex"), (r"//?[^\s()<>\[\]{}/%]+", "variable.other.name"), (r"[{}\[\]]|<<|>>", "punctuation.section")],
    groups=[("keyword.control", "if ifelse for forall repeat loop exit stop stopped begin end def bind load store exec quit"),
            ("support.function", "exch dup pop copy index roll clear count mark cleartomark counttomark add sub mul div idiv mod neg abs sqrt sin cos atan exp ln log round truncate floor ceiling eq ne gt ge lt le and or not xor true false null array dict string length get put getinterval putinterval known where currentdict userdict systemdict cvi cvr cvs cvn cvx cvlit type moveto lineto rmoveto rlineto curveto rcurveto arc arcn arct closepath newpath currentpoint stroke fill eofill clip eoclip gsave grestore setgray setrgbcolor setcmykcolor sethsbcolor setlinewidth setlinecap setlinejoin setdash translate rotate scale concat matrix findfont scalefont makefont setfont selectfont show ashow widthshow kshow stringwidth charpath showpage copypage image imagemask colorimage print pstack stack ==")],
    numbers=r"(?<![^\s()<>\[\]{}/%])(?:\d+#[0-9A-Za-z]+|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?)(?![^\s()<>\[\]{}/%])")

grammar("inform6", "Inform 6", ["i6", "inform"], "inform6", """
Inform 6, Graham Nelson's language for interactive fiction (the Z-machine and Glulx): ! comments, "strings" with
^ ~ @ escapes, 'dictionary words', [ routines ], the directives, Object and Class definitions with their with,
has and class parts, and the statements.""",
    comments=[{"line": r"!.*$"}],
    strings=[{"begin": '"', "end": '"', "escape": r"[\^~]|@@\d+|@\{[0-9A-Fa-f]+\}|@[\w:'`\"^~]{1,2}", "multiline": True}, {"begin": "'", "end": "'", "escape": r"\^|//\w*"}],
    rules=[(r"\[\s*([A-Za-z_]\w*)", {1: "entity.name.function"}),
           (r"(?i)^\s*(object|class|nearby)\s+(?:->\s*)*([A-Za-z_]\w*)", {1: "keyword.declaration", 2: "entity.name.type"})],
    groups=[("keyword.control", "if else switch while do until for objectloop return rtrue rfalse print print_ret new_line give move remove jump break continue box font style spaces inversion read quit restart restore save to in with has class private"),
            ("keyword.other.directive", "constant global array object class attribute property verb extend include release serial statusline replace abbreviate ifdef ifndef ifnot iftrue iffalse endif default stub zcharacter fake_action import link lowstring message switches system_file trace"),
            ("constant.language", "true false nothing self sender noun second actor location player")],
    numbers=r"(?<![\w.])(?:\$\$?[0-9A-Fa-f]+|\d+)")

grammar("zil", "ZIL (Zork Implementation Language)", ["zil", "mdl"], "zil", """
ZIL, Infocom's Zork Implementation Language, and the MDL it comes from: ; comments that comment out the form after
them, "strings" with \\ escapes, <FORMS> in angle brackets, ,global and .local values, and the ROUTINE, OBJECT,
ROOM, GLOBAL, SYNTAX and COND forms.""",
    ci=False, word=r"[\w\-?!]",
    comments=[{"line": r';\s*"(?:[^"\\]|\\.)*"'}, {"line": r";\s*<[^<>]*>"}, {"line": r";\S*"}],
    strings=[{"begin": '"', "end": '"', "escape": r"\\.", "multiline": True}],
    rules=[(r"<\s*(ROUTINE|OBJECT|ROOM|GLOBAL|CONSTANT|SYNTAX|DEFINE|DEFMAC|PROPDEF|SETG)\s+([\w\-?!.]+)", {1: "keyword.declaration", 2: "entity.name.function"}),
           (r",[\w\-?!]+", "variable.other.global"), (r"\.[\w\-?!]+", "variable.other.local"), (r"[<>()\[\]]", "punctuation.section")],
    before=r"(?<=<)\s*",
    groups=[("keyword.control", "COND AND OR NOT REPEAT RETURN RTRUE RFALSE AGAIN PROG BIND TELL CRLF CR PRINTI PRINTD PRINTN PRINTB SET SETG EQUAL? VERB? PRSO? PRSI? FSET FCLEAR FSET? IN? MOVE REMOVE GETP PUTP GET PUT LOC FIRST? NEXT? QUEUE ENABLE DISABLE PERFORM APPLY GOTO JIGS-UP RANDOM PROB ZERO? L? G? MAPF MAPR")],
    numbers=r"(?<![\w\-])-?\d+")
