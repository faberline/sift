"""Synthetic controls for exact source-content comparison."""
import copy, json, tempfile, unittest
from pathlib import Path
from compare_exports import compare, SIGNALS

class Exports(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory(prefix='sift-export-controls-')
        self.addCleanup(self.temp.cleanup); self.root=Path(self.temp.name)
        self.header={'schema':'sift.production-observations/v1','origin':'sift','source_commit':'a'*40,
            'image':'ghcr.io/faberline/sift@sha256:'+'b'*64,'cluster_identity_sha256':'c'*64,
            'start_unix_nano':100,'end_unix_nano':200,'services':['checkout'],
            'complete':True,'sampled':False,'truncated':False,'record_counts':{s:1 for s in SIGNALS}}
        self.rows=[{'signal':signal,'identity':{'project':'p','environment':'prod','source_id':signal+'-one'},
            'content':{'resource':{'pod_uid':'pod-a'},'value':[1,2,3]}} for signal in SIGNALS]
    def file(self,name,header=None,rows=None):
        path=self.root/name
        path.write_text('\n'.join(json.dumps(value) for value in [header or self.header]+(rows or self.rows))+'\n')
        return path
    def test_equal_exports_compare_every_signal_without_claiming_live_duration(self):
        result=compare(self.file('before'),self.file('after'),'recovery')
        self.assertEqual(result['result'],'matched');self.assertIs(result['production_accepted'],False)
        self.assertEqual(result['compared_records'],{s:1 for s in SIGNALS})
    def test_generated_input_can_verify_a_fresh_install_without_gcp_traffic(self):
        header=copy.deepcopy(self.header);header['origin']='generated'
        before=self.file('generated',header=header)
        result=compare(before,self.file('sift'),'generated')
        self.assertEqual(result['result'],'matched')
        self.assertEqual(result['reference_origin'],'generated')
        self.assertIs(result['production_accepted'],False)
        rows=copy.deepcopy(self.rows);rows[3]['content']['value']=[9]
        self.assertEqual(compare(before,self.file('sift-changed',rows=rows),'generated')['differences']['alerts']['changed'],1)
        with self.assertRaises(ValueError):compare(self.file('sift-before'),self.file('sift-after'),'generated')
        with self.assertRaises(ValueError):compare(before,self.file('sift-after'),'dual-write')
    def test_equal_row_counts_cannot_hide_changed_source_identity_or_content(self):
        rows=copy.deepcopy(self.rows);rows[0]['identity']['source_id']='other';rows[1]['content']['value']=[9]
        result=compare(self.file('before'),self.file('after',rows=rows),'recovery')
        self.assertEqual(result['result'],'different')
        self.assertEqual(result['differences']['logs'],{'missing':1,'extra':1,'changed':0})
        self.assertEqual(result['differences']['metrics']['changed'],1)
    def test_truncation_sampling_bad_counts_and_changed_candidate_are_refused(self):
        before=self.file('before')
        for key,value in [('sampled',True),('complete',False),('truncated',True),('image','other'),
                          ('record_counts',{s:0 for s in SIGNALS}),('start_unix_nano',True)]:
            header=copy.deepcopy(self.header);header[key]=value
            with self.subTest(key=key):
                with self.assertRaises(ValueError):compare(before,self.file('after',header=header),'recovery')
    def test_duplicates_limits_and_wrong_origin_are_refused(self):
        before=self.file('before')
        with self.assertRaises(ValueError):compare(before,self.file('after',rows=self.rows+[self.rows[0]]),'recovery')
        with self.assertRaises(ValueError):compare(before,self.file('after'), 'recovery',maximum=4)
        with self.assertRaises(ValueError):compare(before,self.file('after'), 'dual-write')
        header=copy.deepcopy(self.header);header['origin']='gcp'
        self.assertEqual(compare(self.file('gcp',header=header),self.file('sift'),'dual-write')['result'],'matched')

if __name__=='__main__':unittest.main()
