import json,pathlib,sqlite3,sys,time
root=pathlib.Path(sys.argv[1]).parent;c=json.loads(pathlib.Path(sys.argv[1]).read_text());name=c.get('adapter_id',c.get('id','planner'));journal=c.get('journal_name',name)
db=sqlite3.connect(root/(journal+'.sqlite'))
db.executescript('CREATE TABLE IF NOT EXISTS results(id TEXT PRIMARY KEY,input TEXT,response TEXT);CREATE TABLE IF NOT EXISTS attempts(id TEXT);')
for line in sys.stdin:
 p=json.loads(line);m=p['message'];kind=m['type']
 if kind=='hello':reply={'type':'manifest','manifest':{'protocol_version':1,'adapter_id':name,'adapter_version':'1','capabilities':[{'id':'solution.propose','contract':{'kind':'reason','emits':[c.get('target_schema_id','arcs.plan_candidate.v1')]},'required_permissions':['reason.compute']}]}}
 elif kind=='reason_v2':
  inv=m['invocation'];key=inv['invocation_id'];raw=json.dumps(inv,sort_keys=True);revision=inv['constraints'].get('task_replanning',{}).get('revision',0)
  with db:db.execute('INSERT INTO attempts VALUES(?)',(key,))
  old=db.execute('SELECT input,response FROM results WHERE id=?',(key,)).fetchone()
  if old:
   assert old[0]==raw,'planner recovery changed its invocation';reply=json.loads(old[1])
  elif c.get('unavailable') or revision in c.get('fail_revisions',[]):
   reply={'type':'error','code':'unavailable','message':c.get('unavailable_message','PRIVATE_PLANNER_ERROR_MUST_STAY_LOCAL'),'retryable':True}
  else:
   assert c.get('context_namespace','task_planning') in inv['constraints'],'missing authorized context'
   payload=json.loads(json.dumps(c.get('revisions',{}).get(str(revision),c['proposal'])))
   if c.get('select_tag'):
    pool=inv['constraints']['task_planning']['templates']
    selected=next(t for t in pool if c['select_tag'] in t.get('reasoner_description_claim',{}).get('description',{}).get('tags',[]))
    payload={'steps':[{'id':'selected','template':selected['id']}]}
   if c.get('select_available_template'):
    availability=next(i['payload'] for i in inv['context'] if i['schema_id']=='arcs.plan_template_availability.v1')
    assert availability['revision']==revision
    selected=next(t for t in availability['templates'] if any(option['availability']=='ready' for option in t['reasoners']))
    payload={'steps':[{'id':'work','template':selected['id']}]}
   if c.get('copy_assessment'):
    payload['evidence_version_ids']=[i['version_id'] for i in inv['context'] if i['schema_id']=='arcs.goal_assessment_candidate.v1']
    assert any(i['payload']=={'goal_status':'achieved','effect':'success'} for i in inv['context']),'goal assessment fields were not selectively exported'
   status='no_solution' if c.get('no_solution') or revision in c.get('no_solution_revisions',[]) else 'solved'
   reply={'type':'reasoning_result_v2','response':{'invocation_id':key,'request_id':inv['request_id'],'status':status,'candidates':[] if status=='no_solution' else [{'schema_id':inv['target_schema_id'],'required_capabilities':c.get('required_capabilities',[]),'referenced_versions':[i['version_id'] for i in inv['context']],'payload':payload}],'trace':{'method':inv['method'],'implementation':'dynamic-plan-peer','version':'1','iterations':c.get('iterations'),'output_tokens':None,'details':{}}}}
   with db:db.execute('INSERT INTO results VALUES(?,?,?)',(key,raw,json.dumps(reply)))
  (root/(journal+'.started')).write_text(key);(root/(journal+'.r'+str(revision)+'.started')).write_text(key)
  while (c.get('gate') and not (root/(journal+'.release')).exists()) or (c.get('gate_revision')==revision and not (root/(journal+'.r'+str(revision)+'.release')).exists()):time.sleep(.003)
 elif kind=='shutdown':reply={'type':'bye'}
 else:raise RuntimeError(kind)
 print(json.dumps({'protocol_version':1,'request_id':p['request_id'],'message':reply}),flush=True)
 if kind=='shutdown':break
