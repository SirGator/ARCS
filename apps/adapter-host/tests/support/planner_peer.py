import json,pathlib,sqlite3,sys,time
root=pathlib.Path(sys.argv[1]).parent;c=json.loads(pathlib.Path(sys.argv[1]).read_text())
db=sqlite3.connect(root/'planner.sqlite')
db.executescript('CREATE TABLE IF NOT EXISTS results(id TEXT PRIMARY KEY,input TEXT,response TEXT);CREATE TABLE IF NOT EXISTS attempts(id TEXT);')
for line in sys.stdin:
 p=json.loads(line);m=p['message'];kind=m['type']
 if kind=='hello':reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':'planner','adapter_version':'1','capabilities':[{'id':'solution.propose','contract':{'kind':'reason','emits':['arcs.plan_candidate.v1']},'required_permissions':['reason.compute']}]}}
 elif kind=='reason_v2':
  inv=m['invocation'];key=inv['invocation_id'];raw=json.dumps(inv,sort_keys=True)
  with db:db.execute('INSERT INTO attempts VALUES(?)',(key,))
  old=db.execute('SELECT input,response FROM results WHERE id=?',(key,)).fetchone()
  if old:
   assert old[0]==raw,'planner recovery changed its invocation';reply=json.loads(old[1])
  else:
   assert 'task_planning' in inv['constraints'],'missing template catalog'
   payload=c['proposal'];status='no_solution' if c.get('no_solution') else 'solved'
   reply={'type':'reasoning_result_v2','response':{'invocation_id':key,'request_id':inv['request_id'],'status':status,'candidates':[] if status=='no_solution' else [{'schema_id':inv['target_schema_id'],'required_capabilities':[],'referenced_versions':[i['version_id'] for i in inv['context']],'payload':payload}],'trace':{'method':inv['method'],'implementation':'dynamic-plan-peer','version':'1','iterations':None,'output_tokens':None,'details':{}}}}
   with db:db.execute('INSERT INTO results VALUES(?,?,?)',(key,raw,json.dumps(reply)))
  (root/'planner.started').write_text(key)
  while c.get('gate') and not (root/'planner.release').exists():time.sleep(.003)
 elif kind=='shutdown':reply={'type':'bye'}
 else:raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown':break
