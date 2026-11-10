#!/usr/bin/env python3
"""Production gates for Sift 0.2.0. Synthetic fixtures never qualify a release."""
import hashlib,json,re,math
from pathlib import Path

GATES=('functional','promql_oracle','control_failover','load_30m','load_24h',
    'node_failure','pvc_restore','gcs_outage','all_disk_restore','source_fidelity','capacity','delivery','external_availability')

SOURCE_SIGNALS=('logs','metrics','traces','alerts')

COMMON={'result','source_commit','image','cluster_identity_sha256','evidence_sha256'}
FIELDS={
 'functional':{},
 'external_availability':{'metric_type':str,'uptime_check_sha256':str,'endpoint_sha256':str,
    'successful_checks':int,'failed_checks':int,'route_evidence_sha256':str},
 'promql_oracle':{'prometheus_version':str,'expressions_compared':int},
 'control_failover':{'control_state_differences':int,'leader_changes':int},
 'node_failure':{'lost_acknowledged_events':int},
 'pvc_restore':{'lost_acknowledged_events':int,'control_state_differences':int},
 'gcs_outage':{'duration_seconds':int,'acknowledged_after_rpo_limit':int,'collector_pending_preserved':bool,'resumed':bool},
 'all_disk_restore':{'control_state_differences':int,'lost_recent_seconds':int,'identity_differences':int},
 'source_fidelity':{'duration_seconds':int,'reference_origin':str,'differences':int,
    'compared_signals':list,'compared_records':dict,'complete':bool,'gcp_rollback_configured':bool},
 'capacity':{'measured_samples':int,'sample_mean_bytes':float,'cost_cap_usd':float,'actual_cost_usd':float,'capacity_exceeded':bool,'pvc_sized_from_samples':bool},
 'delivery':{'webhook_p95_seconds':float,'retry_notification_ids_unchanged':bool,'unknown_recovery_notifications':int,'mute_verified':bool,'hold_verified':bool},
}
LOAD={'duration_seconds':int,'acknowledged_events':int,'offered_events_per_second':int,
 'concurrent_queries':bool,'concurrent_alerts':bool,'log_query_p95_seconds':float,
 'metric_query_p95_seconds':float,'trace_read_p95_seconds':float,'content_differences':int,
 'identity_differences':int,'incomplete_queries':int}
FIELDS['load_30m']=LOAD;FIELDS['load_24h']=LOAD

def exact_keys(value,keys,name):
    require(type(value) is dict and set(value)==set(keys),name+' fields differ')

def typed(value,kind,name):
    if kind is float:
        require(type(value) in (int,float) and math.isfinite(value),name+' must be finite numeric data')
    else: require(type(value) is kind,name+' has the wrong type')

def require(condition,message):
    if not condition: raise ValueError('production receipt refused: '+message)

def digest(value):return isinstance(value,str) and re.fullmatch('[0-9a-f]{64}',value) is not None

def validate(proof,candidate,image):
    exact_keys(proof,{'schema','source_commit','image','topology','cluster_identity_sha256','gates','cleanup'},'acceptance')
    require(re.fullmatch('[0-9a-f]{40}',str(candidate.get('commit',''))) is not None,'candidate source format')
    require(re.fullmatch(r'ghcr\.io/faberline/sift@sha256:[0-9a-f]{64}',image) is not None,'immutable image format')
    exact_keys(proof['cleanup'],{'status'},'cleanup')
    exact_keys(proof['topology'],{'gke_mode','store_voters','control_voters'},'topology')
    typed(proof['topology']['store_voters'],int,'store voters');typed(proof['topology']['control_voters'],int,'control voters')
    require(proof.get('schema')=='sift.production-acceptance/v2','acceptance schema')
    require(proof.get('source_commit')==candidate['commit'],'source commit differs')
    require(proof.get('image')==image and '@sha256:' in image,'candidate image differs')
    require(proof.get('topology')=={'gke_mode':'standard','store_voters':3,'control_voters':3},'fixed Standard GKE topology')
    require(digest(proof.get('cluster_identity_sha256')),'cluster identity binding')
    require(set(proof.get('gates',{}))==set(GATES),'every production gate is required')
    for name in GATES:
        gate=proof['gates'][name]
        exact_keys(gate, COMMON | set(FIELDS[name]), name)
        for field,kind in FIELDS[name].items(): typed(gate[field],kind,name+'.'+field)
        require(gate.get('result')=='passed',name+' has not passed')
        require(gate.get('source_commit')==candidate['commit'] and gate.get('image')==image,name+' candidate differs')
        require(gate.get('cluster_identity_sha256')==proof['cluster_identity_sha256'],name+' cluster differs')
        require(digest(gate.get('evidence_sha256')),name+' evidence hash')
    for name,seconds,events in [('load_30m',1800,18_000_000),('load_24h',86400,864_000_000)]:
        g=proof['gates'][name]
        require(g.get('duration_seconds',0)>=seconds and g.get('acknowledged_events',0)>=events,name+' duration or count')
        require(g.get('offered_events_per_second')==10_000 and g.get('acknowledged_events',0)>=g['duration_seconds']*10_000,name+' sustained throughput')
        require(g.get('concurrent_queries') is True and g.get('concurrent_alerts') is True,name+' concurrent work')
        require(0<g.get('log_query_p95_seconds',999)<=2 and 0<g.get('metric_query_p95_seconds',999)<=2,name+' log or metric latency')
        require(0<g.get('trace_read_p95_seconds',999)<=1,name+' trace latency')
        require(g.get('content_differences')==0 and g.get('identity_differences')==0,name+' exact data comparison')
        require(g.get('incomplete_queries')==0,name+' incomplete queries')
    for name in ['node_failure','pvc_restore']:
        require(proof['gates'][name].get('lost_acknowledged_events')==0,name+' acknowledged data loss')
    for name in ['control_failover','pvc_restore','all_disk_restore']:
        require(proof['gates'][name].get('control_state_differences')==0,name+' control mismatch')
    require(proof['gates']['control_failover'].get('leader_changes',0)>=1,'no control leader change')
    g=proof['gates']['gcs_outage']
    require(g.get('duration_seconds',0)>900 and g.get('acknowledged_after_rpo_limit')==0,'GCS outage must stop acknowledgements')
    require(g.get('collector_pending_preserved') is True and g.get('resumed') is True,'GCS outage collection or recovery')
    g=proof['gates']['all_disk_restore']
    require(0<=g.get('lost_recent_seconds',999)<=900 and g.get('identity_differences')==0,'all-disk restore bound or identity')
    g=proof['gates']['source_fidelity']
    require(g['duration_seconds']>=1800 and g['differences']==0,'30-minute source comparison')
    require(g['reference_origin'] in ('generated','gcp'),'source reference origin')
    require(g['compared_signals']==list(SOURCE_SIGNALS) and g['complete'] is True,'complete source coverage')
    exact_keys(g['compared_records'],SOURCE_SIGNALS,'compared source counts')
    require(all(type(count) is int and count>0 for count in g['compared_records'].values()),'each signal needs compared records')
    require(g['gcp_rollback_configured'] is (g['reference_origin']=='gcp'),'migration rollback or fresh-install reference')
    g=proof['gates']['capacity']
    require(g['measured_samples']>=100 and g['sample_mean_bytes']>0,'at least 100 real samples are required')
    require(g.get('cost_cap_usd',0)>0 and 0<=g.get('actual_cost_usd',-1)<=g['cost_cap_usd'],'cost cap')
    require(g.get('capacity_exceeded') is False and g.get('pvc_sized_from_samples') is True,'capacity or PVC sizing')
    g=proof['gates']['delivery']
    require(0<g.get('webhook_p95_seconds',999)<=60 and g.get('retry_notification_ids_unchanged') is True,'notification latency or identity')
    require(g.get('unknown_recovery_notifications')==0 and g.get('mute_verified') is True and g.get('hold_verified') is True,'notification state semantics')
    g=proof['gates']['external_availability']
    require(g['metric_type']=='monitoring.googleapis.com/uptime_check/check_passed','Cloud Monitoring uptime metric')
    require(all(digest(g[key]) for key in ('uptime_check_sha256','endpoint_sha256','route_evidence_sha256')),
        'uptime check, endpoint, and route evidence bindings')
    require(g['successful_checks']>0 and g['failed_checks']==0,'successful external uptime observations')
    g=proof['gates']['promql_oracle']
    require(g.get('prometheus_version')=='3.5.0' and g.get('expressions_compared',0)>=22,'pinned Prometheus oracle')
    require(proof.get('cleanup',{}).get('status')=='clean','acceptance cleanup')
    return proof

def evidence_bytes(path, name):
    require(path.is_file() and not path.is_symlink(), name+' evidence file is missing')
    raw=path.read_bytes()
    require(0<len(raw)<=1_048_576, name+' evidence size')
    return raw


def verify_external_route(gate, evidence_dir):
    route_raw=evidence_bytes(evidence_dir/'external-route.json', 'external route')
    require(hashlib.sha256(route_raw).hexdigest()==gate['route_evidence_sha256'], 'external route bytes differ')
    route=json.loads(route_raw)
    identity=('source_commit','image','cluster_identity_sha256','endpoint_sha256')
    exact_keys(route, (*identity,'schema','result','observation_sha256','gateway_pod_uids_sha256',
        'complete_window'), 'external route')
    require(route['schema']=='sift.external-route-evidence/v1' and route['result']=='passed'
        and route['complete_window'] is True and digest(route['gateway_pod_uids_sha256']),
        'external route and complete window review required')
    observation_raw=evidence_bytes(evidence_dir/'external-availability-observation.json', 'external observation')
    require(hashlib.sha256(observation_raw).hexdigest()==route['observation_sha256'],
        'external observation bytes differ')
    observation=json.loads(observation_raw)
    require(observation.get('schema')=='sift.external-availability-observation/v1'
        and observation.get('result')=='observed' and observation.get('production_accepted') is False,
        'external observation contract differs')
    require(all(route[key]==gate[key]==observation.get(key) for key in identity),
        'external route or observation identity differs')
    require(all(observation.get(key)==gate[key] for key in (
        'metric_type','uptime_check_sha256','successful_checks','failed_checks')),
        'external observation results differ from the reviewed gate')


def load_proof(path,candidate,image):
    require(path is not None,'--production-acceptance is required for 0.2.0')
    path=Path(path);require(path.is_file() and not path.is_symlink(),'regular acceptance file')
    raw=path.read_bytes();require(0<len(raw)<=32768,'acceptance size')
    proof=json.loads(raw);validate(proof,candidate,image)
    evidence_dir=path.parent/'evidence'
    for name in GATES:
        evidence_raw=evidence_bytes(evidence_dir/(name+'.json'), name)
        require(hashlib.sha256(evidence_raw).hexdigest()==proof['gates'][name]['evidence_sha256'],name+' evidence bytes differ')
        evidence=json.loads(evidence_raw)
        require(evidence=={k:v for k,v in proof['gates'][name].items() if k!='evidence_sha256'},name+' evidence data differs')
    verify_external_route(proof['gates']['external_availability'], evidence_dir)
    return proof,hashlib.sha256(raw).hexdigest()

def validate_release_receipt(receipt,candidate,image):
    exact_keys(receipt,{'schema','complete','result','candidate','gke','evidence','redaction','production'},'release receipt')
    require(receipt.get('schema')=='sift.gke-release-receipt/v2','0.2.0 requires the production receipt')
    require(receipt.get('complete') is True and receipt.get('result')=='passed','release is incomplete')
    require(receipt.get('candidate')==candidate,'candidate receipt differs')
    validate(receipt['production'],candidate,image)
    gke=receipt['gke'];exact_keys(gke,{'backend','run_id','image','image_provenance','functional','cleanup'},'GKE receipt')
    require(gke['backend']=='gcp' and gke['image']==image and gke['image_provenance']=='prebuilt','GKE candidate image binding')
    exact_keys(receipt['evidence'],{'production_acceptance_sha256'},'release evidence')
    require(digest(receipt['evidence']['production_acceptance_sha256']),'release evidence digest')
    require(gke['run_id']=='prod-'+candidate['commit'][:12]+'-'+receipt['evidence']['production_acceptance_sha256'][:12],'production run binding')
    require(gke['functional']=={name:'passed' for name in GATES} and gke['cleanup']=={'status':'clean'},'production gate or cleanup summary')
    require(receipt['redaction']=={name:False for name in ['kubeconfig_retained','token_retained','secret_retained','cluster_identity_retained','command_output_retained']},'redaction fields')
    require(all(type(value) is bool for value in receipt['redaction'].values()),'redaction types')

def self_test():
    import copy
    c={'commit':'a'*40};image='ghcr.io/faberline/sift@sha256:'+'b'*64
    p={'schema':'sift.production-acceptance/v2','source_commit':c['commit'],'image':image,
        'cluster_identity_sha256':'c'*64,'topology':{'gke_mode':'standard','store_voters':3,'control_voters':3},
        'cleanup':{'status':'clean'},'gates':{}}
    for name in GATES:p['gates'][name]={'result':'passed','source_commit':c['commit'],'image':image,
        'cluster_identity_sha256':'c'*64,'evidence_sha256':'d'*64}
    for name,duration in [('load_30m',1800),('load_24h',86400)]:p['gates'][name].update({
        'duration_seconds':duration,'acknowledged_events':duration*10000,'offered_events_per_second':10000,
        'concurrent_queries':True,'concurrent_alerts':True,'log_query_p95_seconds':1,'metric_query_p95_seconds':1,
        'trace_read_p95_seconds':.5,'content_differences':0,'identity_differences':0,'incomplete_queries':0})
    for name in ['node_failure','pvc_restore']:p['gates'][name]['lost_acknowledged_events']=0
    for name in ['control_failover','pvc_restore','all_disk_restore']:p['gates'][name]['control_state_differences']=0
    p['gates']['control_failover']['leader_changes']=1
    p['gates']['gcs_outage'].update({'duration_seconds':1000,'acknowledged_after_rpo_limit':0,'collector_pending_preserved':True,'resumed':True})
    p['gates']['all_disk_restore'].update({'lost_recent_seconds':900,'identity_differences':0})
    p['gates']['source_fidelity'].update({'duration_seconds':1800,'reference_origin':'generated',
        'differences':0,'compared_signals':list(SOURCE_SIGNALS),'compared_records':{signal:100 for signal in SOURCE_SIGNALS},
        'complete':True,'gcp_rollback_configured':False})
    p['gates']['capacity'].update({'measured_samples':100,'sample_mean_bytes':1000,'cost_cap_usd':100,'actual_cost_usd':90,'capacity_exceeded':False,'pvc_sized_from_samples':True})
    p['gates']['delivery'].update({'webhook_p95_seconds':30,'retry_notification_ids_unchanged':True,'unknown_recovery_notifications':0,'mute_verified':True,'hold_verified':True})
    p['gates']['promql_oracle'].update({'prometheus_version':'3.5.0','expressions_compared':22})
    p['gates']['external_availability'].update({
        'metric_type':'monitoring.googleapis.com/uptime_check/check_passed','uptime_check_sha256':'e'*64,
        'endpoint_sha256':'f'*64,'successful_checks':3,'failed_checks':0,'route_evidence_sha256':'7'*64})
    validate(p,c,image)
    migration=copy.deepcopy(p)
    migration['gates']['source_fidelity'].update({'reference_origin':'gcp','gcp_rollback_configured':True})
    validate(migration,c,image)
    negative=0
    for name in GATES:
        bad=copy.deepcopy(p);del bad['gates'][name]
        try:validate(bad,c,image)
        except ValueError:negative+=1
        else:raise AssertionError(name+' omission accepted')
        bad=copy.deepcopy(p);bad['gates'][name]['image']=image+'bad'
        try:validate(bad,c,image)
        except ValueError:negative+=1
        else:raise AssertionError(name+' image accepted')
    for name,field,value in [('load_24h','acknowledged_events',863999999),('gcs_outage','acknowledged_after_rpo_limit',1),
        ('capacity','measured_samples',99),('external_availability','successful_checks',0),
        ('external_availability','failed_checks',1),('external_availability','endpoint_sha256','invalid'),
        ('external_availability','uptime_check_sha256','invalid'),
        ('external_availability','metric_type','local-readiness'),
        ('all_disk_restore','lost_recent_seconds',901),('delivery','unknown_recovery_notifications',1),
        ('source_fidelity','duration_seconds',1799),('source_fidelity','reference_origin','unknown'),
        ('source_fidelity','complete',False),('source_fidelity','differences',1),
        ('source_fidelity','compared_records',{signal:0 for signal in SOURCE_SIGNALS}),
        ('source_fidelity','compared_records',{signal:True for signal in SOURCE_SIGNALS}),
        ('source_fidelity','compared_records',{'logs':100}),
        ('source_fidelity','compared_signals',['logs','metrics','traces']),
        ('source_fidelity','gcp_rollback_configured',True)]:
        bad=copy.deepcopy(p);bad['gates'][name][field]=value
        try:validate(bad,c,image)
        except ValueError:negative+=1
        else:raise AssertionError(field+' invalid value accepted')
    bad=copy.deepcopy(migration);bad['gates']['source_fidelity']['gcp_rollback_configured']=False
    try:validate(bad,c,image)
    except ValueError:negative+=1
    else:raise AssertionError('migration without rollback accepted')
    bad=copy.deepcopy(p);bad['schema']='sift.production-acceptance/v1'
    try:validate(bad,c,image)
    except ValueError:negative+=1
    else:raise AssertionError('legacy acceptance silently reinterpreted')
    for name,field,value in [('load_30m','log_query_p95_seconds',True),('capacity','actual_cost_usd',float('nan')),
        ('node_failure','lost_acknowledged_events',False),('functional','bearer_token','must-not-be-published')]:
        bad=copy.deepcopy(p);bad['gates'][name][field]=value
        try:validate(bad,c,image)
        except ValueError:negative+=1
        else:raise AssertionError(field+' unsafe value accepted')
    # Exercise the real 0.2 generator with exact manifest and evidence bytes.
    import importlib.util, tempfile
    spec=importlib.util.spec_from_file_location('receipt_generator',Path(__file__).with_name('make-gke-release-receipt.py'))
    generator=importlib.util.module_from_spec(spec);spec.loader.exec_module(generator)
    with tempfile.TemporaryDirectory() as temporary:
        root=Path(temporary);(root/'evidence').mkdir()
        manifest=generator.fixture_candidate('sift','0.2.0',c['commit'])
        raw=(json.dumps(manifest,sort_keys=True)+'\n').encode()
        manifest_path=root/'candidate.json';manifest_path.write_bytes(raw)
        candidate=generator.validate_candidate('sift',raw,manifest)
        image='ghcr.io/faberline/sift@'+candidate['root_digest']
        p['image']=image
        external=p['gates']['external_availability']
        external['image']=image
        identity={key:external[key] for key in ('source_commit','image','cluster_identity_sha256','endpoint_sha256')}
        observation={'schema':'sift.external-availability-observation/v1','result':'observed','production_accepted':False,
            **identity, **{key:external[key] for key in (
                'metric_type','uptime_check_sha256','successful_checks','failed_checks')}}
        observation_raw=json.dumps(observation).encode()
        (root/'evidence'/'external-availability-observation.json').write_bytes(observation_raw)
        route={'schema':'sift.external-route-evidence/v1','result':'passed',**identity,
            'observation_sha256':hashlib.sha256(observation_raw).hexdigest(),
            'gateway_pod_uids_sha256':'8'*64,'complete_window':True}
        route_raw=json.dumps(route).encode()
        route_path=root/'evidence'/'external-route.json'
        route_path.write_bytes(route_raw)
        external['route_evidence_sha256']=hashlib.sha256(route_raw).hexdigest()
        for name,gate in p['gates'].items():
            gate['image']=image
            data=(json.dumps({k:v for k,v in gate.items() if k!='evidence_sha256'},sort_keys=True)+'\n').encode()
            (root/'evidence'/(name+'.json')).write_bytes(data)
            gate['evidence_sha256']=hashlib.sha256(data).hexdigest()
        proof_path=root/'acceptance.json';proof_path.write_text(json.dumps(p)+'\n')
        receipt=generator.make_receipt('sift','gcp',manifest_path,root/'sift-gke-receipt.json',production_acceptance=proof_path)
        validate_release_receipt(receipt,candidate,image)
        route_path.write_bytes(route_raw+b' ')
        try:load_proof(proof_path,candidate,image)
        except ValueError:negative+=1
        else:raise AssertionError('changed external route evidence accepted')
        route_path.write_bytes(route_raw)
        for field,value in [('schema','sift.gke-release-receipt/v1'),('complete',1),('result','failed')]:
            bad=copy.deepcopy(receipt);bad[field]=value
            try:validate_release_receipt(bad,candidate,image)
            except ValueError:negative+=1
            else:raise AssertionError('unsafe release '+field+' accepted')
        try:generator.make_receipt('sift','gcp',manifest_path,root/'sift-gke-receipt.json')
        except SystemExit:negative+=1
        else:raise AssertionError('0.2 legacy evidence accepted')
        evidence=root/'evidence'/'load_24h.json'
        original=evidence.read_bytes();evidence.write_bytes(original+b' ')
        try:load_proof(proof_path,candidate,image)
        except ValueError:negative+=1
        else:raise AssertionError('changed evidence bytes accepted')
        evidence.unlink()
        try:load_proof(proof_path,candidate,image)
        except ValueError:negative+=1
        else:raise AssertionError('missing raw evidence accepted')
    print(f'production receipt self-test PASS: 3 synthetic positive, {negative} negative controls')

if __name__=='__main__': self_test()
