#!/usr/bin/env python3
"""Read the exact GKE deployment before load tests. This does not deploy."""
import argparse, hashlib, json, subprocess
from datetime import datetime, timezone
from pathlib import Path
import prepare

def require(condition, message):
    if not condition: raise ValueError(message)

def inspect(binding, resource, preflight, cluster, nodes, pods):
    image='ghcr.io/faberline/sift@'+binding['root_digest']
    spec=resource['spec']; namespace=resource['metadata']['namespace']; name=resource['metadata']['name']
    require(resource['kind']=='Sift' and spec['image']==image, 'prepared image differs')
    require(spec['replicasPerShard']==3 and spec['voterCount']==3 and spec['auth']=='kubernetes', 'fixed topology or auth differs')
    require(preflight['source_commit']==binding['commit'] and preflight['image']==image
        and preflight['production_accepted'] is False, 'preparation candidate differs')
    identity={'project':spec['gcpProjectId'],'cluster':spec['gkeClusterName'],'location':spec['gkeLocation'],
        'self_link':cluster['selfLink'],'cluster_id':cluster['id']}
    require(cluster['name']==identity['cluster'] and cluster['location']==identity['location']
        and '/projects/'+identity['project']+'/' in cluster['selfLink'], 'live cluster differs')
    require(cluster['status']=='RUNNING' and cluster.get('autopilot',{}).get('enabled',False) is False,
        'Standard GKE must be running')
    require(cluster.get('networkConfig',{}).get('datapathProvider')=='ADVANCED_DATAPATH'
        and cluster.get('enableFqdnNetworkPolicy') is True, 'network policy requirements differ')
    require(cluster.get('workloadIdentityConfig',{}).get('workloadPool')==identity['project']+'.svc.id.goog',
        'live Workload Identity differs')
    identity_hash=hashlib.sha256(json.dumps(identity,sort_keys=True,separators=(',',':')).encode()).hexdigest()
    require(identity_hash==preflight['cluster_identity_sha256'], 'permanent cluster identity differs')
    ready_nodes={n['metadata']['name']:n for n in nodes['items'] if not n.get('spec',{}).get('unschedulable',False)
        and any(c.get('type')=='Ready' and c.get('status')=='True' for c in n.get('status',{}).get('conditions',[]))}
    require(len(ready_nodes)>=3, 'three distinct ready nodes are required')
    selected=[p for p in pods['items'] if p['metadata'].get('namespace')==namespace
        and p['metadata'].get('labels',{}).get('app.kubernetes.io/name')=='sift'
        and p['metadata']['labels'].get('app.kubernetes.io/instance')==name]
    require(selected, 'no candidate Pods found')
    roles={role:[] for role in ['store','control']}
    for pod in selected:
        require(not pod['metadata'].get('deletionTimestamp'), 'candidate Pod is being replaced')
        require(pod['spec'].get('nodeName') in ready_nodes, 'candidate Pod has no ready node')
        containers=[c for c in pod['spec']['containers'] if c['name']=='sift']
        require(len(containers)==1 and containers[0]['image']==image, 'mixed candidate image')
        statuses=[c for c in pod.get('status',{}).get('containerStatuses',[]) if c['name']=='sift']
        require(len(statuses)==1 and statuses[0].get('ready') is True, 'candidate container is not ready')
        architecture=ready_nodes[pod['spec']['nodeName']]['metadata'].get('labels',{}).get('kubernetes.io/arch')
        require(architecture in ('amd64','arm64'), 'candidate node architecture is unsupported')
        runtime_digest=statuses[0].get('imageID','').rsplit('@',1)[-1]
        require(runtime_digest in [binding['root_digest'],binding[architecture+'_digest']], 'runtime image digest differs')
        role=pod['metadata']['labels'].get('sift.axiom.dev/role')
        if role in roles:
            roles[role].append(pod['spec']['nodeName'])
            secrets={v.get('secret',{}).get('secretName') for v in pod['spec'].get('volumes',[])}
            require(spec['peerTlsSecret'] in secrets, 'peer TLS Secret is not mounted')
            if role=='control':require(spec['alerting']['webhookSecret'] in secrets, 'Webhook Secret is not mounted')
    for role,placed in roles.items():
        require(len(placed)==3 and len(set(placed))==3, role+' requires three voters on distinct nodes')
    gateways=[p for p in selected if p['metadata']['labels'].get('sift.axiom.dev/role')=='gateway']
    queries=[p for p in selected if p['metadata']['labels'].get('sift.axiom.dev/role')=='query']
    require(gateways and queries, 'ready gateway and query roles are required')
    gateway=sorted(gateways,key=lambda p:p['metadata']['name'])[0]['metadata']
    require(type(gateway.get('uid')) is str and gateway['uid'], 'gateway Pod identity is missing')
    return {'schema':'sift.production-runtime-check/v1','result':'ready_for_load_tests','production_accepted':False,
        'source_commit':binding['commit'],'image':image,'cluster_identity_sha256':identity_hash,
        'gateway_pod':{'name':gateway['name'],'uid':gateway['uid']},
        'store_voters':3,'control_voters':3,'candidate_pods':len(selected)}

def command(args):
    result=subprocess.run(args,check=True,capture_output=True,timeout=60)
    require(0<len(result.stdout)<=16*1024*1024, 'cluster read exceeds its output limit')
    return json.loads(result.stdout)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for name in ['candidate-manifest','prepared-dir','kubeconfig','output']:parser.add_argument('--'+name,type=Path,required=True)
    args=parser.parse_args()
    raw,candidate=prepare.read(args.candidate_manifest)
    binding=prepare.candidate_module.validate_candidate('sift',raw,candidate)
    _,resource=prepare.read(args.prepared_dir/'instance.json')
    _,preflight=prepare.read(args.prepared_dir/'preflight.json')
    spec=resource['spec']; namespace=resource['metadata']['namespace']
    cluster=command(['gcloud','container','clusters','describe',spec['gkeClusterName'],'--location',spec['gkeLocation'],
        '--project',spec['gcpProjectId'],'--format=json'])
    kube=['kubectl','--kubeconfig='+str(args.kubeconfig),'--request-timeout=30s']
    nodes=command(kube+['get','nodes','-o','json'])
    pods=command(kube+['get','pods','--namespace',namespace,'-o','json'])
    report=inspect(binding,resource,preflight,cluster,nodes,pods)
    report['observed_at']=datetime.now(timezone.utc).isoformat().replace('+00:00','Z')
    require(not args.output.exists(), 'output must be new')
    with args.output.open('x') as out:out.write(json.dumps(report,indent=2)+'\n')
    args.output.chmod(0o600)
    print('Runtime checks passed. Load, fault, and source comparison gates have not run.')

if __name__=='__main__':
    try:main()
    except (ValueError,KeyError,TypeError,subprocess.SubprocessError) as error:raise SystemExit('runtime check refused: '+str(error))
