import json,sys,urllib.request
d=json.load(urllib.request.urlopen('http://127.0.0.1:47240/api/leaderboard'))
b=d['bots']
def e(x): return x.get('elo') if isinstance(x,dict) else x
def g(x): return x.get('games') if isinstance(x,dict) else None
for r in b:
    bot=r['bot']; bid=bot['id'] if isinstance(bot,dict) else bot
    if r['rank']<=12 or 'wildcard' in str(bid):
        print(r['rank'], bid, round(r['elo']), 'duel', round(e(r['duel']) or 0), g(r['duel']), 'ffa', round(e(r['ffa']) or 0), g(r['ffa']), 'slow', r['slowest_ms'])
