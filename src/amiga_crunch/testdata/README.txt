Test files for Limoniter's Amiga decrunchers, made for Limoniter on 20 November 2026 by crunching files of its own
(text.txt, the start of FEATURES.md; sound.raw, a made-up 8-bit wave; hello.exe, assembled by Limoniter from
hello.s) with the real crunchers, run in FS-UAE (Kickstart 3.1):

  text.pp         PowerPacker 2.3a's CLI Crunch (efficiency 1)
  text.imp        Imploder 4.0's FImp (its own choice of mode)
  text.crm        CrunchMania 1.91's CrMData -1c (its normal algorithm)
  text.crm2       CrunchMania 1.91's CrMData -2c (LZ-Huffman)
  sound.crms      CrunchMania 1.91's CrMData -1sc (normal algorithm, sample mode)
  sound.crms2     CrunchMania 1.91's CrMData -2sc (LZ-Huffman, sample mode)
  text.stc        StoneCracker 4.10.3's stc -fd (S404 data)
  hello.stc       StoneCracker 4.10.3's stc -fe (executable, decruncher built in)
  hello.stclib    StoneCracker 4.10.3's stc -fel (executable, decruncher in stc.library)
  hello.imp       Imploder 4.0, normal mode, compression mode 0
  hello.implib    Imploder 4.0, library mode, compression mode 8
  hello.crm       CrunchMania 1.91, reloc mode, LZ-Huffman, normal decruncher
  hello.crmlib    CrunchMania 1.91, reloc mode, LZ-Huffman, library decruncher

The crunched programs carry the crunchers' own small decrunchers, as every file they crunch does; they're here only
to test that Limoniter gives back what was crunched. The crunchers themselves aren't included.
