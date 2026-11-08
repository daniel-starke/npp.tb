# Third-Party Notices

`NppToolBox.dll` contains the following third-party works. They are used under the MIT license,
except for the works marked with a BSD or ISC license and the icons of Font Awesome Free, see
below. The terms of the MIT, BSD and ISC licenses are reproduced at the end. Crates offered under a
choice of licenses (MIT, Apache-2.0, Unlicense, Zlib or 0BSD) are used under the MIT license.

## Embedded data

| Work                                                                       | Used for                                                      | Copyright                                                                       |
|----------------------------------------------------------------------------|---------------------------------------------------------------|---------------------------------------------------------------------------------|
| [github-markdown-css](https://github.com/sindresorhus/github-markdown-css) | HTML style sheets                                             | Copyright (c) Sindre Sorhus <sindresorhus@gmail.com> (https://sindresorhus.com) |
| [Primer Octicons](https://github.com/primer/octicons)                      | Alert icons                                                   | Copyright (c) 2026 GitHub Inc.                                                  |
| [gemoji](https://github.com/github/gemoji)                                 | Emoji shortcodes                                              | Copyright (c) 2019 GitHub, Inc.                                                 |
| [Mermaid](https://github.com/mermaid-js/mermaid) 12.2.0 (compiled themes)  | Mermaid diagram themes and architecture icons                 | Copyright (c) 2014 - 2022 Knut Sveidqvist                                       |
| [Font Awesome Free](https://fontawesome.com) 6.7.2 icons                   | Mermaid diagram icons (feature `font-awesome`, on by default) | Copyright 2024 Fonticons, Inc.                                                  |

The icons of Font Awesome Free are licensed under the
[CC BY 4.0 license](https://creativecommons.org/licenses/by/4.0/)
(<https://fontawesome.com/license/free>). Their paths are stored in a compact binary format.

## Ported code

The Mermaid renderer is a Rust port of parts of the following JavaScript and C++ works, so that its
pictures match those of Mermaid in the browser.

| Work                                                                                   | Ported for                                          | Copyright                                                               |
|----------------------------------------------------------------------------------------|-----------------------------------------------------|-------------------------------------------------------------------------|
| [Mermaid](https://github.com/mermaid-js/mermaid) 12.2.0                                | Parsers, layouts and renderers of all diagram types | Copyright (c) 2014 - 2022 Knut Sveidqvist                               |
| [dagre-d3-es](https://github.com/tbo47/dagre-es) 7.0.14 (dagre, graphlib)              | Layered layout (`dagre`, base of the `elk` layout)  | Copyright (c) 2012-2014 Chris Pettitt, Copyright (c) 2013 Chris Pettitt |
| [cytoscape](https://github.com/cytoscape/cytoscape.js) 3.34                            | Graph parts of the architecture layout              | Copyright (c) 2016-2026, The Cytoscape Consortium.                      |
| [cytoscape-cose-bilkent](https://github.com/cytoscape/cytoscape.js-cose-bilkent) 4.1.0 | Mindmap layout                                      | Copyright (c) 2016-2018, The Cytoscape Consortium.                      |
| [cytoscape-fcose](https://github.com/iVis-at-Bilkent/cytoscape.js-fcose) 2.2.0         | Architecture layout                                 | Copyright (c) 2018 - present, iVis-at-Bilkent.                          |
| [cose-base](https://github.com/iVis-at-Bilkent/cose-base) 1.0.3, 2.2.0                 | Mindmap and architecture layouts                    | Copyright (c) 2019 - present, iVis@Bilkent.                             |
| [layout-base](https://github.com/iVis-at-Bilkent/layout-base) 1.0.2, 2.0.1             | Mindmap and architecture layouts                    | Copyright (c) 2019 iVis@Bilkent                                         |
| [khroma](https://github.com/fabiospampinato/khroma) 2.1.0                              | Theme colors                                        | Copyright (c) 2019-present Fabio Spampinato, Andrew Maney               |
| [roughjs](https://github.com/rough-stuff/rough) 4.6.6                                  | `handDrawn` look                                    | Copyright (c) 2019 Preet Shihn                                          |
| [hachure-fill, points-on-curve, path-data-parser](https://github.com/pshihn)           | `handDrawn` look                                    | Copyright (c) 2020, 2023 Preet Shihn                                    |
| [@upsetjs/venn.js](https://github.com/upsetjs/venn.js) 2.0.0                           | Venn layout                                         | Copyright (c) 2013 Ben Frederickson, Copyright (c) 2021 Samuel Gratzl   |
| [fmin](https://github.com/benfred/fmin) (BSD-3-Clause), as bundled by venn.js          | Venn layout                                         | Copyright 2016, Ben Frederickson                                        |
| [dayjs](https://github.com/iamkun/dayjs) 1.11                                          | Gantt dates                                         | Copyright (c) 2018-present, iamkun                                      |
| [d3-sankey](https://github.com/d3/d3-sankey) 0.12.3 (BSD-3-Clause)                     | Sankey layout                                       | Copyright 2015, Mike Bostock                                            |
| [d3](https://github.com/d3/d3) modules (ISC): d3-shape, d3-path, d3-scale, d3-array    | Edge curves, scales and ticks                       | Copyright 2010-2023 Mike Bostock                                        |
| d3 modules (ISC): d3-format, d3-time, d3-time-format, d3-hierarchy                     | Number and date formats, time axes, treemap layout  | Copyright 2010-2022 Mike Bostock                                        |
| [Chromium](https://www.chromium.org) Blink and Skia (BSD-3-Clause)                     | MathML layout of math in labels, SVG bounding boxes | Copyright 2015 The Chromium Authors, Copyright (c) 2011 Google Inc.     |

## Rust crates

| Crate                                                                                                               | Copyright                                                           |
|---------------------------------------------------------------------------------------------------------------------|---------------------------------------------------------------------|
| [pulldown-cmark](https://github.com/pulldown-cmark/pulldown-cmark)                                                  | Copyright 2015 Google Inc. All rights reserved.                     |
| [bitflags](https://github.com/bitflags/bitflags)                                                                    | Copyright (c) 2014 The Rust Project Developers                      |
| [memchr](https://github.com/BurntSushi/memchr)                                                                      | Copyright (c) 2015 Andrew Gallant                                   |
| [unicase](https://github.com/seanmonstar/unicase)                                                                   | Copyright (c) 2014-2026 Sean McArthur                               |
| [unicode-width](https://github.com/unicode-rs/unicode-width)                                                        | Copyright (c) 2015 The Rust Project Developers                      |
| [windows-sys, windows-core, windows-result, windows-strings, windows-link](https://github.com/microsoft/windows-rs) | Copyright (c) Microsoft Corporation.                                |
| [resvg, usvg](https://github.com/linebender/resvg)                                                                  | Copyright 2017 the Resvg Authors                                    |
| [tiny-skia, tiny-skia-path](https://github.com/linebender/tiny-skia) (BSD-3-Clause)                                 | Copyright (c) 2011 Google Inc., Copyright (c) 2020 Yevhenii Reizner |
| [kurbo, polycool](https://github.com/linebender/kurbo)                                                              | Copyright (c) 2018 Raph Levien                                      |
| [svgtypes](https://github.com/linebender/svgtypes)                                                                  | Copyright (c) 2018 Yevhenii Reizner                                 |
| [simplecss](https://github.com/linebender/simplecss)                                                                | Copyright (c) 2018 Reizner Evgeniy                                  |
| [roxmltree](https://github.com/RazrFalcon/roxmltree)                                                                | Copyright (c) 2018 Yevhenii Reizner                                 |
| [strict-num](https://github.com/RazrFalcon/strict-num)                                                              | Copyright (c) 2022 Yevhenii Reizner                                 |
| [pico-args](https://github.com/RazrFalcon/pico-args)                                                                | Copyright (c) 2019 Yevhenii Reizner                                 |
| [data-url](https://github.com/servo/rust-url)                                                                       | Copyright (c) 2013-2025 The rust-url developers                     |
| [imagesize](https://github.com/Roughsketch/imagesize)                                                               | Copyright (c) 2017 Maiddog                                          |
| [float-cmp](https://github.com/mikedilger/float-cmp)                                                                | Copyright (c) 2014-2020 Optimal Computing (NZ) Ltd                  |
| [siphasher](https://github.com/jedisct1/rust-siphash)                                                               | Copyright (c) Frank Denis                                           |
| [smallvec](https://github.com/servo/rust-smallvec)                                                                  | Copyright (c) 2018 The Servo Project Developers                     |
| [arrayvec](https://github.com/bluss/arrayvec)                                                                       | Copyright (c) Ulrik Sverdrup "bluss" 2015-2023                      |
| [arrayref](https://github.com/droundy/arrayref) (BSD-2-Clause)                                                      | Copyright (c) 2015 David Roundy                                     |
| [bytemuck](https://github.com/Lokathor/bytemuck)                                                                    | Copyright (c) 2019 Daniel "Lokathor" Gee.                           |
| [rgb](https://github.com/kornelski/rust-rgb)                                                                        | Copyright (c) 2019 Kornel                                           |
| [log](https://github.com/rust-lang/log)                                                                             | Copyright (c) 2014 The Rust Project Developers                      |
| [cfg-if](https://github.com/rust-lang/cfg-if)                                                                       | Copyright (c) 2014 Alex Crichton                                    |

Only with the build feature `pure-rust`:

| Crate                                                            | Copyright                                                   |
|------------------------------------------------------------------|-------------------------------------------------------------|
| [png](https://github.com/image-rs/image-png)                     | Copyright (c) 2015 nwin                                     |
| [fdeflate](https://github.com/image-rs/fdeflate)                 | Copyright (c) The image-rs Developers                       |
| [flate2](https://github.com/rust-lang/flate2-rs)                 | Copyright (c) 2014-2026 Alex Crichton                       |
| [miniz_oxide](https://github.com/Frommi/miniz_oxide)             | Copyright 2013-2014 RAD Game Tools and Valve Software       |
| [adler2](https://github.com/oyvindln/adler2)                     | Copyright (c) Jonas Schievink, oyvindln                     |
| [crc32fast](https://github.com/srijs/rust-crc32fast)             | Copyright (c) 2018 Sam Rijs, Alex Crichton and contributors |
| [simd-adler32](https://github.com/mcountryman/simd-adler32)      | Copyright (c) 2021 Marvin Countryman                        |
| [gif](https://github.com/image-rs/image-gif)                     | Copyright (c) 2015 nwin                                     |
| [weezl](https://github.com/image-rs/weezl)                       | Copyright (c) HeroicKatora 2020                             |
| [color_quant](https://github.com/image-rs/color_quant)           | Copyright (c) 2016 PistonDevelopers                         |
| [image-webp](https://github.com/image-rs/image-webp)             | Copyright (c) The image-rs Developers                       |
| [byteorder-lite](https://github.com/image-rs/byteorder-lite)     | Copyright (c) 2015 Andrew Gallant                           |
| [quick-error](https://github.com/tailhook/quick-error)           | Copyright (c) 2015 The quick-error Developers               |
| [zune-jpeg, zune-core](https://github.com/etemesi254/zune-image) | Copyright (c) zune-image developers                         |

## MIT License

Permission is hereby granted, free of charge, to any person obtaining a copy of this software
and associated documentation files (the "Software"), to deal in the Software without
restriction, including without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or
substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## BSD-3-Clause License (tiny-skia, tiny-skia-path)

Copyright (c) 2011 Google Inc. All rights reserved.
Copyright (c) 2020 Yevhenii Reizner All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

- Redistributions of source code must retain the above copyright notice, this list of conditions
  and the following disclaimer.
- Redistributions in binary form must reproduce the above copyright notice, this list of
  conditions and the following disclaimer in the documentation and/or other materials provided
  with the distribution.
- Neither the name of the copyright holder nor the names of its contributors may be used to
  endorse or promote products derived from this software without specific prior written
  permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

## BSD-3-Clause License (d3-sankey, fmin, Chromium)

Copyright 2015, Mike Bostock (d3-sankey). Copyright 2016, Ben Frederickson (fmin).
Copyright 2015 The Chromium Authors (Chromium). All rights reserved.

The terms are the same as those of the BSD-3-Clause license above.

## ISC License (d3 modules)

Copyright 2010-2023 Mike Bostock

Permission to use, copy, modify, and/or distribute this software for any purpose with or without
fee is hereby granted, provided that the above copyright notice and this permission notice appear
in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS
SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE
AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT,
NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR PERFORMANCE OF
THIS SOFTWARE.

## BSD-2-Clause License (arrayref)

Copyright (c) 2015 David Roundy <roundyd@physics.oregonstate.edu>
All rights reserved.

Redistribution and use in source and binary forms, with or without modification, are permitted
provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions
   and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of
   conditions and the following disclaimer in the documentation and/or other materials provided
   with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR
IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND
FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT
OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
