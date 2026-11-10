#!/usr/bin/env python3
"""Offline controls for the acceptance preparation entry point."""
import copy, json, unittest
import prepare

class Preparation(unittest.TestCase):
    def setUp(self):
        self.candidate=prepare.candidate_module.fixture_candidate('sift','0.2.0','a'*40)
        self.raw=json.dumps(self.candidate).encode()
        self.config={'project':'project-one','cluster':'acceptance','location':'asia-east1-a',
            'namespace':'app','name':'sift','archive_uri':'gs://project-one-backups/sift',
            'peer_tls_secret':'sift-peer-tls','webhook_secret':'sift-webhooks','webhook_hosts':['alerts.example.com'],
            'services':[{'project':'project-one','namespace':'app','container':'checkout'}],
            'reference_origin':'generated','comparison_hours':.5,
            'cost_cap_usd':1000,'estimated_hourly_cost_usd':1,'estimated_fixed_cost_usd':10,
            'measured_store_bytes_per_item':1000,'store_pvc_gib':50000,'control_pvc_gib':5}
        self.cluster={'name':'acceptance','location':'asia-east1-a','status':'RUNNING','id':'permanent-cluster-id',
            'selfLink':'https://container.googleapis.com/v1/projects/project-one/zones/asia-east1-a/clusters/acceptance',
            'networkConfig':{'datapathProvider':'ADVANCED_DATAPATH','enableFqdnNetworkPolicy':True},
            'workloadIdentityConfig':{'workloadPool':'project-one.svc.id.goog'}}
        self.samples=[{'resource':{'type':'k8s_container','labels':{'project_id':'project-one',
            'namespace_name':'app','container_name':'checkout','cluster_name':'acceptance',
            'location':'asia-east1-a'}},'textPayload':'a real input shape'}]*100
    def run_prepare(self):
        return prepare.prepare(self.raw,self.candidate,self.config,self.cluster,self.samples)
    def test_fixed_topology_and_immutable_image_never_claim_live_acceptance(self):
        resource,report=self.run_prepare()
        self.assertEqual(resource['spec']['replicasPerShard'],3)
        self.assertEqual(resource['spec']['voterCount'],3)
        self.assertIn('@sha256:',resource['spec']['image'])
        self.assertIs(report['production_accepted'],False)
        self.assertEqual(report['budget']['estimated_hours'],25)
        self.assertEqual(report['budget']['estimated_cost_usd'],35)
        self.assertEqual(report['source_comparison'],{'reference_origin':'generated','minimum_hours':.5,'gcp_rollback_required':False})
        self.assertNotIn('seven-day dual-write',report['required_live_checks'])
        self.assertEqual(report['local_hot_retention_seconds'],2592000)
        self.assertEqual(report['minimum_store_pvc_gib'],48280)
    def test_missing_spend_capacity_or_destination_controls_are_refused(self):
        original=copy.deepcopy(self.config)
        for key,value in [('cost_cap_usd',0),('cost_cap_usd',1),('measured_store_bytes_per_item',0),
            ('store_pvc_gib',1),('store_pvc_gib',50),('control_pvc_gib',True),('webhook_hosts',['*.example.com']),
            ('archive_uri','gs://project-one-backups'),('estimated_hourly_cost_usd',float('nan')),
            ('reference_origin','unknown'),('comparison_hours',.49),('comparison_hours',True),
            ('comparison_hours',float('nan'))]:
            with self.subTest(key=key,value=value):
                self.config=copy.deepcopy(original);self.config[key]=value
                with self.assertRaises(ValueError):self.run_prepare()
    def test_migration_window_is_explicit_and_budgeted_without_a_seven_day_minimum(self):
        self.config.update({'reference_origin':'gcp','comparison_hours':1,'cost_cap_usd':35.5})
        _,report=self.run_prepare()
        self.assertEqual(report['budget']['estimated_hours'],25.5)
        self.assertEqual(report['budget']['estimated_cost_usd'],35.5)
        self.assertTrue(report['source_comparison']['gcp_rollback_required'])
        self.config['cost_cap_usd']=35.49
        with self.assertRaises(ValueError):self.run_prepare()
    def test_system_or_other_project_samples_cannot_size_applications(self):
        for key,value in [('namespace_name','kube-system'),('project_id','another-project'),('container_name','other'),
                          ('cluster_name','another-cluster'),('location','another-location')]:
            with self.subTest(key=key):
                sample=copy.deepcopy(self.samples[0]);sample['resource']['labels'][key]=value
                self.samples=[sample]*100
                with self.assertRaises(ValueError):self.run_prepare()
        self.config['services'][0]['namespace']='gke-managed-cim'
        self.samples[0]['resource']['labels']['namespace_name']='gke-managed-cim'
        with self.assertRaises(ValueError):self.run_prepare()
    def test_source_outside_the_agent_scope_is_refused(self):
        for field,value in [('project','another-project'),('namespace','another-namespace')]:
            original=copy.deepcopy(self.config)
            with self.subTest(field=field):
                self.config['services'][0][field]=value
                with self.assertRaises(ValueError):self.run_prepare()
            self.config=original
    def test_wrong_cluster_and_autopilot_are_refused(self):
        for key,value in [('name','other'),('autopilot',{'enabled':True})]:
            original=copy.deepcopy(self.cluster)
            with self.subTest(key=key):
                self.cluster[key]=value
                with self.assertRaises(ValueError):self.run_prepare()
            self.cluster=original
    def test_nested_fqdn_network_policy_is_required(self):
        self.cluster['enableFqdnNetworkPolicy']=True
        for fqdn_fields in ({'enableFqdnNetworkPolicy':False}, {}):
            with self.subTest(fqdn_fields=fqdn_fields):
                self.cluster['networkConfig']={'datapathProvider':'ADVANCED_DATAPATH',**fqdn_fields}
                with self.assertRaisesRegex(ValueError,'FQDN network policy must be enabled'):
                    self.run_prepare()

if __name__=='__main__': unittest.main()
