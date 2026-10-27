/* All businesses, jobs, counts, prices and timelines below are FICTIONAL demo data. */
window.AskHavaSamples = Object.freeze({
  requestId:'1042', demoCode:'123456',
  quotes:[
    {id:'a',name:'Sample Plumbing Co.',type:'firm',price:285,kind:'FIRM PRICE',words:'I can replace the kitchen faucet and check the supply lines for $285, including labor. Any hidden damage would need a new quote.',includes:'Labor, common fittings, leak check',arrival:'Tomorrow, 9 a.m.–noon',expires:'October 29',license:'ROC #000000 (sample)',licenseRequired:true,spanish:true,afterHours:false,received:'Received first'},
    {id:'b',name:'Sample Desert Repair',type:'range',min:240,max:360,kind:'PRICE RANGE',words:'Most faucets like this come out between $240 and $360. I would confirm after seeing the connections in person.',includes:'Inspection and standard installation labor',arrival:'Thursday afternoon',expires:'October 30',license:'ROC #000000 (sample)',licenseRequired:true,spanish:false,afterHours:true,received:'Received second'},
    {id:'c',name:'Sample Home Service',type:'trip',price:65,kind:'TRIP FEE + ESTIMATE ON SITE',words:'Our trip fee is $65. We will give a separate repair estimate before starting any repair work.',includes:'On-site visit only; repair not included',arrival:'Friday morning',expires:'October 30',license:'ROC #000000 (sample)',licenseRequired:true,spanish:false,afterHours:false,received:'Received third'},
    {id:'d',name:'Sample Pool Care',type:'monthly',price:145,kind:'MONTHLY PRICE',words:'For routine weekly pool cleaning, our monthly service is $145. Filters and repairs cost extra.',includes:'Four routine visits per month',arrival:'Next Monday',expires:'October 31',license:'No state contractor license shown for this example',licenseRequired:false,spanish:true,afterHours:false,received:'Received fourth'}
  ],
  jobCards:[
    {id:1072,title:'AC not cooling',area:'86403',timing:'Today',status:'Waiting for a quote',tier:75},
    {id:1073,title:'Weekly pool service',area:'86404',timing:'This week',status:'Waiting for a quote',tier:25},
    {id:1074,title:'Water heater replacement',area:'86406',timing:'Within three days',status:'Picked — confirm by 4:15 p.m.',tier:199}
  ],
  activity:[{title:'Kitchen faucet',status:'Quotes ready',id:1042},{title:'Pool cleanout',status:'Business confirmed',id:1037},{title:'Window cleaning',status:'Done',id:992}]
});
