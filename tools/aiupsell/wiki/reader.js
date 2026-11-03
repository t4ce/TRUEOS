// Local reading demo for Hull, not a reproduction of Wikipedia's live UI.
// Run through Hull's :paste / :end console after a local preview has loaded.
var body = document.querySelector('body');
body.textContent = '';
body.setAttribute('style', 'margin:0;padding:42px 54px;background:#101822;color:#e5eff6;font-family:sans-serif;font-size:21px;line-height:1.45');
function text(tag, content, style) {
 var node = document.createElement(tag);
 node.textContent = content;
 node.setAttribute('style', style || '');
 body.appendChild(node);
 return node;
}
text('p', 'WIKIPEDIA / LOCAL READING DEMO', 'color:#76e9d2;font-size:17px;margin:0 0 20px');
text('h1', 'Artificial intelligence', 'font-size:42px;margin:0 0 18px;color:#ffffff');
text('p', 'Computers that learn, reason and solve problems. AI connects computer science, mathematics and engineering to build systems that can act toward a goal.', 'margin:0 0 22px');
var ai = text('a', 'Read Artificial intelligence on Wikipedia', 'color:#76e9d2;font-size:19px');
ai.setAttribute('href', 'https://en.wikipedia.org/wiki/Artificial_intelligence');
text('h2', 'Sam Altman', 'font-size:32px;margin:30px 0 12px;color:#ffffff');
text('p', 'Entrepreneur and investor. Co-founder of OpenAI in 2015, with earlier work at Loopt and Y Combinator.', 'margin:0 0 18px');
var sam = text('a', 'Read Sam Altman on Wikipedia', 'color:#76e9d2;font-size:19px');
sam.setAttribute('href', 'https://en.wikipedia.org/wiki/Sam_Altman');
text('p', 'HULL ON TRUEOS / Locally prepared summaries, not the full live Wikipedia pages.', 'font-size:16px;color:#a6bacb;margin-top:30px');
