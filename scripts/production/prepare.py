#!/usr/bin/env python3
"""Prepare local GKE acceptance inputs. This command does not deploy resources."""
import argparse, hashlib, json, math, re, sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'release'))
import importlib.util
module_spec = importlib.util.spec_from_file_location('candidate', Path(__file__).resolve().parents[1] / 'release/make-gke-release-receipt.py')
candidate_module = importlib.util.module_from_spec(module_spec)
module_spec.loader.exec_module(candidate_module)

def require(condition, message):
    if not condition: raise ValueError(message)

def read(path, bound=16*1024*1024):
    path=Path(path)
    require(path.is_file() and not path.is_symlink(), 'input must be a regular file')
    raw=path.read_bytes();require(0<len(raw)<=bound, 'input size exceeds its limit')
    return raw,json.loads(raw)

def number(value, name):
    require(type(value) in (int,float) and math.isfinite(value) and value>0, name+' must be positive')
    return value

def prepare(candidate_raw, candidate, config, cluster, entries):
    binding=candidate_module.validate_candidate('sift',candidate_raw,candidate)
    require(binding['version']=='0.2.0','this acceptance plan is for 0.2.0')
    image='ghcr.io/faberline/sift@'+binding['root_digest']
    required={'project','cluster','location','namespace','name','archive_uri','peer_tls_secret','webhook_secret',
        'webhook_hosts','services','cost_cap_usd','estimated_hourly_cost_usd','estimated_fixed_cost_usd',
        'measured_store_bytes_per_item','store_pvc_gib','control_pvc_gib','reference_origin','comparison_hours'}
    require(type(config) is dict and set(config)==required, 'acceptance configuration fields differ')
    require(config['reference_origin'] in ('generated','gcp'),'source reference must be generated or GCP')
    comparison_hours=number(config['comparison_hours'],'source comparison hours')
    require(comparison_hours>=.5,'source comparison must cover at least 30 minutes')
    for name in ['namespace','name','peer_tls_secret','webhook_secret']:
        require(re.fullmatch('[a-z0-9](?:[-a-z0-9]{0,61}[a-z0-9])?',config[name]) is not None, name+' must be a Kubernetes name')
    require(cluster.get('name')==config['cluster'] and cluster.get('location')==config['location'], 'cluster differs')
    require('/projects/'+config['project']+'/' in cluster.get('selfLink',''), 'cluster project differs')
    require(cluster.get('status')=='RUNNING' and cluster.get('autopilot',{}).get('enabled',False) is False, 'Standard GKE must be running')
    network=cluster.get('networkConfig',{})
    require(network.get('datapathProvider')=='ADVANCED_DATAPATH', 'GKE Dataplane V2 is required')
    require(network.get('enableFqdnNetworkPolicy') is True, 'FQDN network policy must be enabled')
    require(cluster.get('workloadIdentityConfig',{}).get('workloadPool')==config['project']+'.svc.id.goog', 'Workload Identity is required')
    require(re.fullmatch(r'gs://[a-z0-9][a-z0-9._-]+/[^?#\s]+',config['archive_uri']) is not None,'GCS prefix is required')
    hosts=config['webhook_hosts']
    require(type(hosts) is list and 0<len(hosts)<=32 and len(set(hosts))==len(hosts), 'Webhook host list is invalid')
    for host in hosts:
        require(type(host) is str and len(host)<=253 and re.fullmatch(r'[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+',host) is not None,'Webhook hosts must be exact DNS names')
    services=config['services']
    require(type(services) is list and len(services)>0,'test or application services are required')
    for service in services:
        require(type(service) is dict and set(service)=={'project','namespace','container'}, 'service source fields differ')
        require(all(type(value) is str and value for value in service.values()),'service identity is required')
        require(not service['namespace'].startswith(('kube-','gmp-','gke-')),'system logs cannot size application storage')
        require(service['project']==config['project'],'application source project must match the prepared cluster')
        require(service['namespace']==config['namespace'],
            'current agents collect the instance namespace; application sources must use that namespace')
    selected=[]
    require(type(entries) is list,'Cloud Logging sample must be a JSON array')
    for entry in entries:
        resource=entry.get('resource',{});labels=resource.get('labels',{})
        if resource.get('type')!='k8s_container':continue
        if labels.get('cluster_name')!=config['cluster'] or labels.get('location')!=config['location']:continue
        if any(labels.get('project_id')==s['project'] and labels.get('namespace_name')==s['namespace']
            and labels.get('container_name')==s['container'] for s in services):
            selected.append(entry)
    require(len(selected)>=100,'at least 100 matching test or application samples are required')
    require(all(any(e['resource']['labels'].get('project_id')==s['project']
        and e['resource']['labels'].get('namespace_name')==s['namespace']
        and e['resource']['labels'].get('container_name')==s['container'] for e in selected) for s in services), 'each service needs samples')
    sizes=sorted(len(json.dumps(entry,ensure_ascii=False,separators=(',',':')).encode()) for entry in selected)
    footprint=number(config['measured_store_bytes_per_item'],'measured store bytes per item')
    # Archive commits compact the WAL. Local segments still have 30-day retention.
    # Size every replica for the rated steady-state window, with twofold reserve.
    local_hot_seconds=30*24*60*60
    minimum_store_gib=math.ceil(footprint*10_000*local_hot_seconds*2/(1024**3))
    for key in ['store_pvc_gib','control_pvc_gib']:
        require(type(config[key]) is int and config[key]>0,key+' must be a positive integer')
    require(config['store_pvc_gib']>=minimum_store_gib,'store PVC is below the measured local footprint with twofold reserve')
    cap=number(config['cost_cap_usd'],'cost cap')
    hourly=number(config['estimated_hourly_cost_usd'],'estimated hourly cost')
    fixed=config['estimated_fixed_cost_usd']
    require(type(fixed) in (int,float) and math.isfinite(fixed) and fixed>=0,'fixed cost estimate is invalid')
    # The operator supplies current rates for nodes, disks, GCS, and request costs.
    hours=24.5+comparison_hours
    estimate=hours*hourly+fixed
    require(estimate<=cap,'load and source comparison estimate exceeds the cost cap')
    identity={key:config[key] for key in ['project','cluster','location']}
    identity['self_link']=cluster['selfLink']
    require(type(cluster.get('id')) is str and 0<len(cluster['id'])<=512,'permanent GKE cluster identity is required')
    identity['cluster_id']=cluster['id']
    cluster_hash=hashlib.sha256(json.dumps(identity,sort_keys=True,separators=(',',':')).encode()).hexdigest()
    resource={'apiVersion':'sift.axiom.dev/v1alpha1','kind':'Sift',
        'metadata':{'name':config['name'],'namespace':config['namespace']},
        'spec':{'image':image,'peerTlsSecret':config['peer_tls_secret'],'replicasPerShard':3,'voterCount':3,
            'auth':'kubernetes','archive':{'destination':config['archive_uri']},
            'alerting':{'webhookSecret':config['webhook_secret'],'allowedHosts':hosts},
            'storage':{'storeSize':str(config['store_pvc_gib'])+'Gi','controlSize':str(config['control_pvc_gib'])+'Gi','gatewaySize':'2Gi','querySize':'2Gi'},
            'ingest':{'maxItemsPerMinute':720000,'maxConcurrentRequests':32},
            'gcpProjectId':config['project'],'gkeClusterName':config['cluster'],'gkeLocation':config['location']}}
    report={'schema':'sift.production-preparation/v2','result':'prepared','production_accepted':False,
        'source_commit':binding['commit'],'image':image,'cluster_identity_sha256':cluster_hash,
        'source_comparison':{'reference_origin':config['reference_origin'],'minimum_hours':comparison_hours,
            'gcp_rollback_required':config['reference_origin']=='gcp'},
        'sample_sources':services,
        'sample_count':len(sizes),'source_entry_mean_bytes':sum(sizes)/len(sizes),
        'source_entry_p95_bytes':sizes[math.ceil(len(sizes)*.95)-1],
        'measured_store_bytes_per_item':footprint,'minimum_store_pvc_gib':minimum_store_gib,
        'local_hot_retention_seconds':local_hot_seconds,
        'budget':{'cap_usd':cap,'estimated_hours':hours,'estimated_cost_usd':estimate},
        'required_live_checks':['three distinct ready nodes','same candidate on all Sift Pods','GCS write and read',
            'Webhook allowlist and Secret mount','GCP external probe','30-minute load','24-hour load',
            'node and PVC loss','GCS outage longer than 15 minutes','all-disk joint restore','complete source comparison']}
    return resource,report

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['candidate-manifest','config','cluster','samples','output-dir']:parser.add_argument('--'+name,required=True,type=Path)
    args=parser.parse_args()
    candidate_raw,candidate=read(args.candidate_manifest)
    _,config=read(args.config);_,cluster=read(args.cluster);sample_raw,samples=read(args.samples,64*1024*1024)
    resource,report=prepare(candidate_raw,candidate,config,cluster,samples)
    report['sample_file_sha256']=hashlib.sha256(sample_raw).hexdigest()
    require(not args.output_dir.exists(),'output directory must be new')
    args.output_dir.mkdir(mode=0o700,parents=True)
    for name,value in [('instance.json',resource),('preflight.json',report)]:
        path=args.output_dir/name;path.write_text(json.dumps(value,indent=2)+'\n');path.chmod(0o600)
    print('Prepared immutable candidate inputs. Live acceptance has not run.')

if __name__=='__main__':
    try:main()
    except (ValueError,KeyError,TypeError) as error:raise SystemExit('preparation refused: '+str(error))
