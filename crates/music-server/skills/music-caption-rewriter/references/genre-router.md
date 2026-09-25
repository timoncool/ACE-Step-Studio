# Genre Router

Use this file as the first disclosure layer. Route the Music Brief to one primary family and, only when useful, one secondary family. Then read only those family indexes.

## Routing contract

1. Normalize explicit genre names and cultural-market terms with the aliases below.
2. Choose the primary family from the user's main genre, not from a generic mood adjective.
3. Add one secondary family only for an explicit fusion or a clearly requested contrasting palette.
4. Read no more than two family indexes for ordinary requests.
5. Within those indexes, prefer a card that already combines both requested styles before mixing separate references.
6. Treat `ballad`, `emotional`, `epic`, `modern`, `dark`, and `cinematic` as modifiers unless the input supplies stronger genre, groove, or instrumentation evidence.

## Family map

| Route | Positive cues | Disambiguation | Index |
|---|---|---|---|
| `east-asian-modern` | Mandopop, C-pop, Cantopop, J-pop with electronic, R&B, hip-hop, dance, funk, rock, or metal production | Use the heritage family for acoustic, orchestral, traditional, or conventional ballad writing | [index-east-asian-modern.md](index-east-asian-modern.md) |
| `east-asian-ballad-heritage` | Mandopop, C-pop, Cantopop, J-pop ballad, guofeng pop, acoustic or orchestral East Asian pop | Add roots/traditional only when traditional instruments are central rather than decorative | [index-east-asian-ballad-heritage.md](index-east-asian-ballad-heritage.md) |
| `modern-rnb-neo-soul` | Contemporary R&B, alternative R&B, neo-soul, trap soul, atmospheric R&B | Lo-fi R&B may also use hip-hop; classic soul belongs to soul/blues/gospel | [index-modern-rnb-neo-soul.md](index-modern-rnb-neo-soul.md) |
| `soul-blues-gospel` | Soul, blues, blues rock, gospel, worship, soul-blues | Route jazz-blues primarily by the user's stated identity and groove | [index-soul-blues-gospel.md](index-soul-blues-gospel.md) |
| `cinematic-pop-ballad` | Cinematic pop, cinematic ballad, orchestral pop song, soundtrack-like vocal ballad | Use cinematic orchestral when score, trailer, orchestra, or choir is the main identity | [index-cinematic-pop-ballad.md](index-cinematic-pop-ballad.md) |
| `cinematic-orchestral-epic` | Film score, orchestral, trailer, epic choral, symphonic soundtrack, contemporary classical | `Cinematic` alone is normally a modifier, not enough to choose this family | [index-cinematic-orchestral-epic.md](index-cinematic-orchestral-epic.md) |
| `electronic-synth-ambient-pop` | Synth-pop, electropop, dream pop, ambient pop, darkwave, retrowave, downtempo | Use club/EDM only when the request emphasizes drops, club grooves, house, trance, or festival energy | [index-electronic-synth-ambient-pop.md](index-electronic-synth-ambient-pop.md) |
| `jazz-swing-big-band` | Vocal jazz, jazz ballad, big band, swing, bossa nova, lounge jazz | Traditional crooner pop without strong jazz ensemble cues may use traditional vocal/stage | [index-jazz-swing-big-band.md](index-jazz-swing-big-band.md) |
| `traditional-vocal-stage` | Traditional pop, crooner, doo-wop, a cappella, musical theatre, show tune, cabaret | Use jazz/swing when the rhythm section or big-band language is dominant | [index-traditional-vocal-stage.md](index-traditional-vocal-stage.md) |
| `hip-hop-rap` | Hip-hop, rap, trap, drill, lo-fi hip-hop, conscious rap, melodic rap | R&B singing over trap drums may use modern R&B as primary and hip-hop as secondary | [index-hip-hop-rap.md](index-hip-hop-rap.md) |
| `metal-heavy-rock` | Metalcore, power metal, symphonic metal, nu-metal, hard rock, post-hardcore | Alternative and pop rock without heavy-metal technique belong to pop/alternative rock | [index-metal-heavy-rock.md](index-metal-heavy-rock.md) |
| `pop-alternative-rock` | Pop rock, alternative rock, indie rock, arena rock, J-rock, punk, post-grunge | Country rock, blues rock, and folk rock follow their roots family when that identity is primary | [index-pop-alternative-rock.md](index-pop-alternative-rock.md) |
| `contemporary-folk-acoustic` | Indie folk, contemporary folk, folk pop, singer-songwriter, modern acoustic pop | Use roots/traditional for heritage, regional, maritime, Celtic, or traditional folk identity | [index-contemporary-folk-acoustic.md](index-contemporary-folk-acoustic.md) |
| `roots-traditional-global` | Traditional folk, Celtic, Chinese traditional, folk blues, reggae, maritime, global folk fusion | Guofeng pop remains East Asian pop when pop songwriting is primary | [index-roots-traditional-global.md](index-roots-traditional-global.md) |
| `general-pop-ballad` | Pop, contemporary pop, pop ballad, broadly described emotional song | Use only as fallback when no more specific genre family is supported | [index-general-pop-ballad.md](index-general-pop-ballad.md) |
| `dance-pop-disco-funk` | Dance-pop, nu-disco, funk-pop, disco revival, groove-led pop | House, trance, hardstyle, and festival drops belong to club/EDM | [index-dance-pop-disco-funk.md](index-dance-pop-disco-funk.md) |
| `club-edm-house-trance` | EDM, house, trance, hardstyle, dubstep, techno, festival electronic | Electronic pop without a club or drop structure belongs to electronic/synth/ambient pop | [index-club-edm-house-trance.md](index-club-edm-house-trance.md) |
| `country-americana` | Country, Americana, bluegrass, country rock, country pop, rockabilly | Folk-country follows the user's primary label; read contemporary folk second only when necessary | [index-country-americana.md](index-country-americana.md) |

## Common aliases

| User wording | Normalize toward |
|---|---|
| 华语流行、国语流行 | Mandopop / C-pop |
| 粤语流行 | Cantopop |
| 国风流行 | East Asian pop, optionally roots/traditional |
| 氛围 R&B、另类节奏布鲁斯 | Alternative R&B |
| 复古电子 | Resolve from synthwave/retrowave versus disco/house cues |
| 朋克流行 | Pop-punk |
| 后摇感 | Alternative rock unless the user explicitly requests post-rock |
| 电影感 | A modifier; choose cinematic as primary only for score-led writing |
| 史诗感 | A modifier; choose cinematic orchestral only with orchestral, trailer, or choral evidence |
| 燃、炸、强烈 | Energy cues, never genre evidence by themselves |

Normalize spelling variants such as `Hip Hop`/`Hip-Hop`, `Dance Pop`/`Dance-Pop`, `Synth Pop`/`Synth-Pop`, and `R&B`/`R'n'B` before routing.

## Fusion rules

- Interpret `X with Y influences` as primary `X`, secondary `Y`.
- Interpret an ordered form such as `X / Y` the same way unless the user explicitly gives both equal weight.
- Read at most two family indexes. Keep a third style as an in-index ranking cue rather than opening a third index.
- Prefer a hybrid card in the primary index that already expresses both styles.
- Use the secondary reference only for its requested dimension: instrumentation, groove, vocal treatment, cultural color, arrangement, or production.
- Never let a secondary reference overwrite explicit genre, tempo, vocal, instrument, or exclusion constraints.

Examples:

- `Mandopop with trap R&B production` → primary `east-asian-modern`, secondary `modern-rnb-neo-soul` or `hip-hop-rap` according to whether singing or beat language dominates.
- `Metalcore with Chinese traditional instruments` → primary `metal-heavy-rock`, secondary `roots-traditional-global`.
- `Cinematic folk ballad` → primary `contemporary-folk-acoustic` when it is a song; primary `cinematic-orchestral-epic` when it is a score.
- `Lo-fi female R&B` → primary `modern-rnb-neo-soul`, secondary `hip-hop-rap` only when a lo-fi beat is central.

## Fallback routing

When the user gives no genre:

1. Use groove evidence such as swing, trap, four-on-the-floor, breakbeat, or acoustic strumming.
2. Then use core instrumentation and vocal delivery.
3. Then use cultural or market context.
4. If only mood or imagery remains, read [index-general-pop-ballad.md](index-general-pop-ballad.md) and keep the result conservative.

Do not route from mood alone when stronger musical evidence exists.
