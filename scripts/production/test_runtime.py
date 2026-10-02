#!/usr/bin/env python3
"""Offline controls for cluster, image, and voter placement checks."""
import copy, unittest
import prepare, test_prepare, verify_runtime

class Runtime(unittest.TestCase):
    def setUp(self):
        inputs=test_prepare.Preparation();inputs.setUp()
        self.binding=prepare.candidate_module.validate_candidate('sift',inputs.raw,inputs.candidate)
        self.resource,self.preflight=inputs.run_prepare();self.cluster=inputs.cluster
        self.nodes={'items':[{'metadata':{'name':'node-'+str(i),'labels':{'kubernetes.io/arch':'amd64'}},
            'status':{'conditions':[{'type':'Ready','status':'True'}]}} for i in range(3)]}
        self.pods={'items':[]}
        for role in ['store','control']:
            for i in range(3):
                self.pods['items'].append({'metadata':{'namespace':'sift-acceptance','labels':{
                    'app.kubernetes.io/name':'sift','app.kubernetes.io/instance':'sift','sift.axiom.dev/role':role}},
                    'spec':{'nodeName':'node-'+str(i),'containers':[{'name':'sift','image':self.resource['spec']['image']}],
                        'volumes':[{'secret':{'secretName':name}} for name in ['sift-peer-tls','sift-webhooks']]},
                    'status':{'containerStatuses':[{'name':'sift','ready':True,'imageID':'registry@'+self.binding['amd64_digest']}]}})
        for role in ['gateway','query']:
            pod=copy.deepcopy(self.pods['items'][0])
            pod['metadata'].update({'name':'sift-'+role+'-abc','uid':role+'-uid'})
            pod['metadata']['labels']['sift.axiom.dev/role']=role
            self.pods['items'].append(pod)
    def inspect(self):
        return verify_runtime.inspect(self.binding,self.resource,self.preflight,self.cluster,self.nodes,self.pods)
    def test_ready_snapshot_does_not_claim_production(self):
        self.assertIs(self.inspect()['production_accepted'],False)
    def test_changed_permanent_cluster_id_is_refused(self):
        self.cluster['id']='new-cluster-with-the-same-name'
        with self.assertRaises(ValueError):self.inspect()
    def test_missing_node_or_voter_is_refused(self):
        self.nodes['items'].pop()
        with self.assertRaises(ValueError):self.inspect()
        self.setUp();self.pods['items'].pop(0)
        with self.assertRaises(ValueError):self.inspect()
        self.setUp();self.pods['items'].pop()
        with self.assertRaises(ValueError):self.inspect()
    def test_same_node_mixed_image_and_missing_secrets_are_refused(self):
        original=copy.deepcopy(self.pods)
        for field,value in [('nodeName','node-0'),('containers',[{'name':'sift','image':'sift:latest'}]),('volumes',[])]:
            self.pods=copy.deepcopy(original);self.pods['items'][1]['spec'][field]=value
            with self.subTest(field=field):
                with self.assertRaises(ValueError):self.inspect()
    def test_unready_or_different_runtime_image_is_refused(self):
        for field,value in [('ready',False),('imageID','registry@sha256:'+'f'*64)]:
            original=copy.deepcopy(self.pods)
            self.pods['items'][0]['status']['containerStatuses'][0][field]=value
            with self.subTest(field=field):
                with self.assertRaises(ValueError):self.inspect()
            self.pods=original

if __name__=='__main__':unittest.main()
