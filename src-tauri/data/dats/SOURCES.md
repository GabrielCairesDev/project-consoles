# DATs No-Intro e Redump — fonte e licença

Arquivos `.dat` (formato clrmamepro) obtidos do repositório
[libretro/libretro-database](https://github.com/libretro/libretro-database),
commit `12c21cf79f1e20eff6ac68b80875816c717d0a7a` (DATs versão `2026.10.07`):

- cartuchos: pasta `metadat/no-intro`, dados do projeto [No-Intro](https://no-intro.org);
- `Sony - PlayStation.dat`: pasta `metadat/redump`, dados do projeto
  [Redump](http://redump.org).

Licença: [Creative Commons Attribution-ShareAlike 4.0](https://creativecommons.org/licenses/by-sa/4.0/)
(CC-BY-SA 4.0). Os arquivos estão sem modificações.

São embutidos no binário (`src/systems.rs`). Os de cartucho identificam ROMs
por tamanho e CRC32; o de PlayStation identifica discos pelo número de série
gravado no `SYSTEM.CNF` (e pelo tamanho da trilha 1, entre revisões). Para
atualizar, baixe os mesmos arquivos de uma versão mais nova do repositório,
troque-os aqui e atualize o commit acima.
