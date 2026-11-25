// Generates the local track used by the CPU scenes: 3 s of digital silence (the audio session is muted
// meanwhile), then a synthetic signal, an embedded cover and enhanced LRC lyrics (one timestamp per word).
//   node scripts/scenes/fixture.mjs <dir>
import { execFileSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const dir = process.argv[2];
if (!dir) throw new Error('usage: fixture.mjs <dir>');
mkdirSync(dir, { recursive: true });
const SECONDS = 300;
const signal = [
  '0.5*exp(-18*mod(t,0.5))*sin(2*PI*55*t)',
  '0.09*sin(2*PI*220*t)+0.08*sin(2*PI*277.18*t)+0.08*sin(2*PI*329.63*t)',
  '0.05*sin(2*PI*1760*t)*exp(-9*mod(t,0.25))',
  '0.05*(2*random(0)-1)*exp(-30*mod(t,0.25))',
].join('+');
const cover = join(dir, 'cover.jpg');
const mp3 = join(dir, 'scene-bench.mp3');
const ffmpeg = (args) => execFileSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', ...args], { stdio: 'inherit' });
ffmpeg(['-f', 'lavfi', '-i', 'gradients=s=1000x1000:c0=0x7a2cff:c1=0xff5a1f:c2=0x12b5a6:x0=0:y0=0:x1=999:y1=999:nb_colors=3:d=1', '-frames:v', '1', '-q:v', '3', cover]);
ffmpeg([
  '-f', 'lavfi', '-i', `aevalsrc='gt(t,3)*(${signal})':d=${SECONDS}:s=44100:c=stereo`,
  '-i', cover,
  '-map', '0:a', '-map', '1:v', '-c:a', 'libmp3lame', '-b:a', '320k', '-c:v', 'copy', '-disposition:v', 'attached_pic',
  '-id3v2_version', '3', '-metadata', 'title=Scene Bench', '-metadata', 'artist=Riff Bench', '-metadata', 'album=Scene Album',
  mp3,
]);

const stamp = (t) => `${String(Math.floor(t / 60)).padStart(2, '0')}:${(t % 60).toFixed(2).padStart(5, '0')}`;
const WORDS = ['silver', 'rivers', 'carry', 'quiet', 'echoes', 'through', 'golden', 'evening', 'light', 'tonight'];
const lines = [];
for (let line = 0; line < (SECONDS - 4) / 4; line++) {
  const start = 4 + line * 4;
  const words = Array.from({ length: 6 }, (_, i) => `<${stamp(start + i * 0.6)}>${WORDS[(line + i) % WORDS.length]}`);
  lines.push(`[${stamp(start)}]${words.join(' ')}`);
}
writeFileSync(join(dir, 'scene-bench.lrc'), `${lines.join('\n')}\n`);

// Lyrics for the visual comparisons: right-to-left, then mixed directions, with combining marks.
const timedLines = (sentences) => sentences.map((sentence, rank) => {
  const start = 2 + rank * 4;
  const words = sentence.split(' ').map((word, i) => `<${stamp(start + i * 0.6)}>${word}`);
  return `[${stamp(start)}]${words.join(' ')}`;
}).join('\n');
const ARABIC = ['أحبك يا حياتي في كل يوم', 'يا ليل طال السهر والقمر فوق المدينة', 'لا تبتعد عني', 'مرحبا بالحب والأمل في قلبي الصغير دائما وأبدا', 'نغني معا تحت السماء'];
const MIXED = ['Hello مرحبا world', 'مرحبا Hello يا صديقي', 'Café déjà vu (encore une fois)', 'Привет, мир и друзья', 'אני אוהב אותך tonight', '夜空の星を見上げて'];
writeFileSync(join(dir, 'scene-arabic.lrc'), `${timedLines(Array.from({ length: 12 }, () => ARABIC).flat())}\n`);
writeFileSync(join(dir, 'scene-mixed.lrc'), `${timedLines(Array.from({ length: 10 }, () => MIXED).flat())}\n`);
