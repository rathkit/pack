import small from './photo.png?w=16&format=webp';
import meta from './photo.png?w=16&as=metadata';
import srcset from './photo.png?w=16;32&format=png&as=srcset';
console.log('url', small.replace(/^.*\//, '').replace(/-[A-Za-z0-9_-]{6,}(?=\.)/, '-HASH'));
console.log('meta', meta.width, meta.format);
console.log('srcset', srcset.split(',').length);
