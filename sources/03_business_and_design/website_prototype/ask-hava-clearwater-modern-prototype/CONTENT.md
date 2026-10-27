# Ask Hava prototype — content inventory

The text below is extracted from every static page generated in this version. Some live status, error and conditional UI strings exist only in `assets/js/flows.js`; that source file is the authoritative dynamic copy appendix. All Spanish text is a working draft pending native-speaker review.

## 404.html — Page not found
- 404
- That page isn’t here.
- The link may have moved, or the address may be wrong.
- Go home

## about.html — About
- About Ask Hava
- A local concierge that does more of the asking around.
- Ask Hava helps people discover Lake Havasu. Gathering service quotes is the next step.
- Local in spirit. Neutral by design.
- Ask Hava is built for Lake Havasu City: places to go, events to enjoy and businesses to find. The quote feature helps you ask several local businesses about a job without chasing calls.
- Our service is not a contractor. We do not do the work, inspect it or endorse one business over another. Quotes are never sponsored or ranked for payment.
- The founder story
- Owner introduction to be added
- Placeholder: add approved founder background and a real portrait only if desired. No invented credentials or photos.

## accessibility.html — Accessibility
- Information
- Accessibility
- We are working toward a website everyone can use.
- Our goal
- Target: WCAG 2.2 Level AA. Accessibility has not yet been independently certified.
- Last review
- October 26, 2026 — code-level checks planned; manual assistive-technology review pending.
- Known issues
- This is a front-end prototype with fake data. External font files need internet access; some visual styling differs offline. Comprehensive screen-reader testing is pending.
- Report an issue
- Email help@askhava.com or call the sample phone number. Target response to accessibility messages: two business days once a staffed help channel is live. This is a planned target, not a current guarantee.
- Handyman services: an unlicensed handyman may only be considered for jobs under $1,000 total where no permit is required.

## app/business-portal.html — Business workspace demo
- Interactive prototype
- Business workspace
- This is a fictional day in the business portal. Try quoting, confirming and changing settings.
- Fictional businesses, jobs and prices. No real homeowners, payment charges or outbound communication.
- Good morning, Sample HVAC Company.
- Requests to quote
- 2
- Sample count
- Picks to confirm
- 1
- Sample count
- Customers to call
- 0
- Sample count
- Today
- Jobs and quotes
- Billing
- Settings
- Needs attention today
- Pick waiting
- Water heater replacement
- Request 1074 · ZIP 86406
- Confirm by 4:15 p.m.
- (sample clock deadline, no countdown)
- Confirm selection (demo)
- New request
- AC not cooling
- Request 1072 · ZIP 86403 · Homeowner details hidden
- Submit a quote in the Jobs tab.
- Job cards
- #1072 · AC not cooling
- 86403 · Today · Waiting for a quote
- #1073 · Weekly pool service
- 86404 · This week · Waiting for a quote
- Send a sample quote
- Price type
- Firm price
- Price range
- Monthly price
- Trip fee + estimate on site
- Amount in USD
- Highest amount in range
- Earliest visit
- Quote valid until
- Your exact quote wording
- Includes a service visit and a written repair plan.
- Estimated Ask Hava fee if selected and confirmed:
- $25
- . Your first win is free. Trip-fee tier is a demo placeholder; real tiers depend on job type.
- Save sample quote
- Sample billing
- Illustrative transactions
- Item
- Amount
- Status
- First chosen job #1037
- $0
- First win free
- Chosen job #1038
- $75
- Paid (sample)
- Unable to contact homeowner #1039
- -$25
- Refund (sample)
- Payment method:
- Placeholder — no card entry or payment system connected.
- View sample statement
- Job and team preferences
- Pause incoming jobs
- Lake Havasu City
- After-hours emergencies
- We speak Spanish
- Staff notification phone (sample only)
- Save demo settings

## app/business-signup.html — Business sign-up demo
- Interactive prototype
- Join Ask Hava for Business
- Set up a sample business account. Every entry stays in your browser; nothing is submitted.
- All license records, identity steps and confirmations are simulated. The sample number is 000000. No government database is queried.
- Tell us about your business
- Business name *
- License path
- State contractor license applies
- Service with no state contractor license requirement
- State license number
- Enter sample number 000000 to see an illustrative license result.
- Look up sample license
- No state contractor license required for this sample category; a city business-license check still applies.
- Continue
- What work do you want?
- A sample R-39 license covers AC repair, heating and system replacement. Other contracting work needs another license. For an unlicensed service, only non-contractor categories are offered.
- Licensed job types
- AC repair
- Heating
- System replacement
- Electrical panel replacement — requires a different license
- Roofing — requires a different license
- Unlicensed-service examples
- Window cleaning
- Car detailing
- House cleaning
- Illustrative city business-license check, not connected to a city database.
- Continue
- Back
- Where and when do you work?
- Towns served
- Lake Havasu City
- Bullhead City (future launch)
- Fort Mohave (future launch)
- Kingman (future launch)
- Job preferences
- After-hours emergencies
- We speak Spanish
- Skip tenant requests
- Also send jobs nobody else covers
- Continue
- Back
- Identity and consent
- We would send a code to the phone on the state record. Choose an alternative if needed.
- Identity-check method
- Text the number on record
- Call with the code
- Email me
- Mail a letter
- We will text the number on the state record (demo only).
- Six-digit identity code (demo: 123456)
- Fee schedule
- Under $500: $25 · $500–$1,500: $75 · Over $1,500: $199 · First win free. No sign-up or monthly fee.
- I agree to receive job notices and account texts from Ask Hava at this number. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help.
- Terms
- ·
- Privacy
- Consent is unchecked by default. This is a simulated sign-up; no SMS is sent.
- Finish demo sign-up
- Back
- Demo setup complete
- Your first win is free.
- Welcome to the sample portal. This prototype has not created an account or verified any license or identity.
- Open sample business portal

## app/owner.html — Owner dashboard demo
- Interactive prototype
- Owner oversight dashboard
- Operations, safety switches and coverage — all fictional sample data.
- All figures, incidents and settings are simulated. No live services can be changed here.
- Operations overview
- Sample operational results
- Metric
- Today
- This week
- This month
- Requests
- 4
- 7
- 26
- Picks
- 1
- 2
- 8
- Paid introductions
- 1
- 2
- 6
- Money in
- $25
- $125
- $475
- Refunds
- $0
- $25
- Net
- $25
- $125
- $450
- Needs you
- Owner review
- Sample unresolved callback request · homeowner asked to speak with a person
- Attention
- Sample service category with no active business coverage
- Pause controls
- Checking a switch simulates a pause; it does not change a live system.
- Pause all requests
- Pause Lake Havasu City
- Pause plumbing
- Pause charging
- Pause texting
- Coverage by category and town
- Covered · green
- Lake Havasu · Plumbing
- Sample coverage sufficient
- Thin · yellow
- Lake Havasu · Boat detail
- Sample limited participation
- No coverage · red
- Kingman · Pool repair
- Future market; no sample coverage

## app/portal.html — My jobs
- Homeowner portal · Demo
- My jobs. My choices.
- Sign into a fictional homeowner account to explore the job timeline and settings.
- Demo only. Do not enter a real phone number. Your entries are not sent or saved on a server.
- Sign in with a phone code
- Phone number (demo placeholder)
- Six-digit code
- Demo code: 123456
- Open sample account
- Welcome to your jobs
- My jobs
- Past jobs
- My info
- Current jobs
- Quotes ready
- Kitchen faucet · Request 1042
- Request received
- Collecting quotes
- Three sample quotes ready
- Open quotes
- Business confirmed
- Pool cleanout · Sample job 1037
- Next: the business will call through the masked number.
- Other sample statuses: Collecting quotes · Picked · Contacted · Done · Closed · Cancelled
- Past jobs
- Done
- Window cleaning · Sample job 992
- Sample Window Care · Independent sample business
- Request this business again
- Repeat work with the same customer is free for the business.
- My info
- Sample property label
- On-site contact
- Preferred language
- English
- Español
- Account texts on
- In the live service you can reply STOP to opt out.
- Save demo settings
- Delete my data (reset demo)

## app/quotes.html — Sample quote comparison
- Private quote page · Sample request 1042
- Every quote. One private page.
- Three fictional plumbing quotes and one separate pool-service example show how the comparison works. Each request is ordered by arrival, not price.
- All businesses, license entries, dates and prices on this page are fictional. No state-license check was performed.
- Quotes ready
- More quotes may arrive
- Kitchen faucet leak
- Lake Havasu City · ZIP 86403 · Request 1042 (sample)
- Quotes appear in arrival order. No paid positions or recommendations.
- A separate example: monthly pool care
- The next sample belongs to a different, recurring-service request. It is not a quote for the kitchen faucet.
- Need to do something else?
- Ask a question
- Report a problem
- Close this request
- Selection confirmed (demo)
- You picked a business.
- has two business hours to confirm. We would text you who will call and from what number. No real message was sent.
- See sample job history
- ×
- Confirm your choice
- You selected
- .
- We’ll text a code to your own phone. Nothing is shared until you enter it. In this demo, no SMS is sent.
- Six-digit code
- Demo code: 123456. Paste works.
- Confirm selection
- ×
- Ask anonymously
- Your question
- Send (demo only)
- ×
- Report a problem
- What went wrong?
- Report (demo only)
- ×
- Close this request?
- This stops the sample request. It is not a cancellation sent to a business. In the real text service, reply CLOSE 1042.
- Yes, close
- Keep open

## app/request.html — Start a request
- Get quotes · Demo
- Tell Hava what you need.
- The steps below are a working demonstration. No text or photo is sent anywhere.
- Prototype only — no business receives requests and no SMS is sent. Do not enter sensitive information.
- First, your permission.
- Ask Hava uses automated texts to send updates about your request.
- Hi, this is Ask Hava. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: askhava.com/terms. Reply OK to start.
- English consent disclosure — pending legal review.
- Terms
- ·
- Privacy
- OK, start (demo only)
- What needs doing?
- Type of job
- Choose a category
- Plumbing
- AC and heating
- Electrical
- Handyman
- Appliance repair
- Garage doors
- Roofing
- Pool cleaning (weekly)
- Pool repair
- Water softeners
- Landscape maintenance
- Pressure washing
- House cleaning
- Vacation-rental turnover cleaning
- Window cleaning
- Trash-can cleaning
- Dog waste pickup
- Pest control
- Home watch
- Junk removal
- Painting
- Solar screens
- Car detailing
- Boat detailing
- Boat repair
- Local movers
- Tell us about the problem *
- For example: The kitchen faucet has a slow leak under the sink.
- This may be an emergency
- Gas or rotten-egg smell: leave immediately, do not use switches, and call 911 or the gas company. Sparks, smoke or burning: leave the area and call 911. Flooding: move to safety and contact emergency services if needed.
- Are you safe now?
- Yes
- No
- After you are safe, what timing do you want?
- Emergency / after-hours (usually costs more)
- Regular hours
- Continue after safety check
- Continue
- Back
- A few details.
- ZIP code *
- How soon?
- As soon as available
- This week
- Flexible
- Do you own or rent?
- I own the property
- I rent
- I will not be there
- Preferred language
- English
- Español
- Who can meet the business?
- Optional job photo
- Demo: file name shown only. No upload occurs.
- Send demo request
- Back
- Demo complete
- Local businesses have your job.
- In the live service, we will text when the first quote arrives. In this demo, nothing was sent.
- View sample quotes

## businesses.html — For businesses
- Ask Hava for Business
- Get local jobs.
- Pay only when you're picked.
- Quote the work you want. If a homeowner chooses you and you confirm, the fee is $25, $75 or $199, depending on job size. Never pay for a quote that loses.
- Join free
- See pricing
- No monthly fee · No contract · Your first win is free
- Sample job card · Anonymous
- AC not cooling
- Lake Havasu · ZIP 86403
- Homeowner request
- Today
- No customer name, address, or phone is shown at the quote stage.
- Only pay if they pick your quote and you confirm.
- How it works for businesses
- 01
- Pick the work you want
- Sign up, choose your services, towns and whether you take emergency requests.
- 02
- Quote from your phone
- Review an anonymous job card, then send your own price and wording. Skip jobs you don't want.
- 03
- Confirm when chosen
- A homeowner chooses you. Confirm, pay your first applicable fee and call through a masked number.
- Try business portal
- Published third-party estimates · October 2026
- What a lead can cost elsewhere
- These outside marketing reports describe typical industry advertising costs, not verified costs for your business. Numbers below were supplied in the planning brief and have not been independently audited for this prototype.
- Industry estimates versus Ask Hava charges
- Trade
- Outside cost per lead, won or lost
- Outside cost per customer
- Ask Hava fee if chosen
- HVAC
- $51–$149
- ~$288
- $25 repair · $199 replacement
- Plumbing
- $57–$183
- ~$263
- $25 repair · $75–$199 larger jobs
- Electrical
- $25–$80
- ~$218
- $25 · $75 larger jobs
- Roofing
- $162–$228
- ~$369
- $75 repair · $199 replacement
- Handyman
- $8–$34
- ~$77
- $25
- Landscaping
- $10–$40
- ~$89
- $25 per client
- Pest control
- $20–$150
- ~$46–$68
- $25 per client
- Sources:
- The Media Captain
- ,
- PipelineOn
- ,
- Webtonic
- . Estimates from marketing-industry reports; your costs may differ.
- A factual listing, not an endorsement
- “Quotes through Ask Hava” simply means your business opted in to receive quote requests. It is not a quality rating, a verification badge or a recommendation.
- Ask Hava’s directory can carry ads. Quote pages never carry ads and never allow paid ranking.
- What happens after a win?
- The customer verifies their choice with a phone code.
- You confirm and the applicable fee is charged. Your first win is free.
- You receive their first name, address and job photos.
- Call through Ask Hava's masked number. Their phone number stays private.
- Create demo business account

## contact.html — Contact
- Contact
- Need a hand?
- Reach Ask Hava for questions, feedback or accessibility problems.
- Get in touch
- help@askhava.com
- (928) 555-0100
- Sample number — inactive
- No contact form or SMS works in this prototype. Nothing you enter will be sent.
- Send a message
- Name
- Email
- Message
- Try form (demo only)

## es/404.html — Página no encontrada
**Borrador — pendiente de revisión por un hablante nativo.**
- 404
- Esta página no existe.
- El enlace pudo cambiar o la dirección es incorrecta.
- Volver al inicio

## es/about.html — Acerca de
**Borrador — pendiente de revisión por un hablante nativo.**
- Acerca de Ask Hava
- Tu conserje local también puede pedir cotizaciones.
- Ask Hava ayuda a conocer Lake Havasu. Reunir cotizaciones de servicios es el siguiente paso.
- Local y neutral por diseño.
- Ask Hava está pensado para Lake Havasu City: lugares, eventos y negocios. La función de cotizaciones te ayuda a consultar a varios negocios sin hacer tantas llamadas.
- No somos contratistas. No realizamos ni inspeccionamos el trabajo, ni respaldamos a un negocio sobre otro. Las cotizaciones no son patrocinadas ni se ordenan por pago.
- La historia del fundador
- Presentación del propietario pendiente
- Marcador de posición: incluir información verificada del fundador y una foto real solo si se desea. No se inventan credenciales ni fotos.

## es/accessibility.html — Accesibilidad
**Borrador — pendiente de revisión por un hablante nativo.**
- Información
- Accesibilidad
- Trabajamos para que todas las personas puedan usar este sitio.
- Nuestro objetivo
- Objetivo: WCAG 2.2 nivel AA. La accesibilidad aún no cuenta con certificación independiente.
- Última revisión
- October 26, 2026 — revisión de código planificada; pruebas manuales con tecnología de asistencia pendientes.
- Problemas conocidos
- Es un prototipo con datos ficticios. Las fuentes externas necesitan conexión; el estilo puede variar sin internet. Las pruebas exhaustivas con lectores de pantalla están pendientes.
- Reportar un problema
- Envía un correo a help@askhava.com o llama al número de ejemplo. Respuesta prevista para mensajes de accesibilidad: dos días hábiles cuando haya soporte activo. Esto es una meta futura, no una garantía actual.
- Reparaciones menores: una persona sin licencia de contratista solo puede considerarse para trabajos menores de $1,000 en total y que no requieren permiso.

## es/app/portal.html — Mis trabajos
**Borrador — pendiente de revisión por un hablante nativo.**
- Portal para propietarios · Demo
- Mis trabajos. Mis decisiones.
- Accede a una cuenta ficticia para explorar el historial de trabajos y la configuración.
- Solo demostración. No ingreses un número real. No se envía ni guarda información en un servidor.
- Entrar con código telefónico
- Teléfono (de ejemplo)
- Código de seis dígitos
- Código de demo: 123456
- Abrir cuenta de ejemplo
- Bienvenido a tus trabajos
- Mis trabajos
- Trabajos anteriores
- Mis datos
- Trabajos actuales
- Cotizaciones listas
- Grifo de cocina · Solicitud 1042
- Solicitud recibida
- Reuniendo cotizaciones
- Tres cotizaciones de ejemplo listas
- Abrir cotizaciones
- Negocio confirmado
- Limpieza de piscina · Trabajo 1037
- Siguiente: el negocio llamará por medio del número protegido.
- Otros estados de ejemplo: Reuniendo · Elegido · Contactado · Terminado · Cerrado · Cancelado
- Trabajos anteriores
- Terminado
- Limpieza de ventanas · Trabajo 992
- Sample Window Care · Negocio ficticio independiente
- Solicitar el mismo negocio
- Los trabajos repetidos con el mismo cliente son gratuitos para el negocio.
- Mis datos
- Nombre de propiedad de ejemplo
- Persona en el lugar
- Idioma preferido
- English
- Español
- Mensajes de cuenta activados
- En el servicio real puedes responder STOP para darte de baja.
- Guardar configuración ficticia
- Eliminar mis datos (reiniciar demo)

## es/app/quotes.html — Comparación de cotizaciones
**Borrador — pendiente de revisión por un hablante nativo.**
- Página privada · Solicitud ficticia 1042
- Todas las cotizaciones. Una página privada.
- Tres cotizaciones ficticias de plomería y un ejemplo separado de piscina muestran cómo comparar. Cada solicitud usa el orden de llegada, no el precio.
- Todos los negocios, licencias, fechas y precios de esta página son ficticios. No se verificaron licencias.
- Cotizaciones listas
- Pueden llegar más
- Fuga en grifo de cocina
- Lake Havasu City · Código 86403 · Solicitud 1042 (ejemplo)
- Las cotizaciones aparecen en orden de llegada. No hay puestos pagados ni recomendaciones.
- Otro ejemplo: servicio mensual de piscina
- La siguiente cotización pertenece a otra solicitud de servicio recurrente. No es una cotización para el grifo de cocina.
- ¿Necesitas hacer algo más?
- Hacer una pregunta
- Reportar un problema
- Cerrar esta solicitud
- Elección confirmada (demo)
- Elegiste un negocio.
- tiene dos horas hábiles para confirmar. Te avisaríamos quién te llamará y desde qué número. No se envió ningún mensaje real.
- Ver historial de ejemplo
- ×
- Confirma tu elección
- Elegiste
- .
- Enviaríamos un código a tu teléfono. Nada se comparte hasta que lo ingreses. En esta demostración no hay SMS.
- Código de seis dígitos
- Código de prueba: 123456. Puedes pegarlo.
- Confirmar elección
- ×
- Preguntar anónimamente
- Tu pregunta
- Enviar (solo demo)
- ×
- Reportar un problema
- ¿Qué ocurrió?
- Reportar (solo demo)
- ×
- ¿Cerrar esta solicitud?
- Esto cierra la solicitud de ejemplo. No envía cancelaciones. En el servicio de SMS real, responde CLOSE 1042.
- Sí, cerrar
- Mantener abierta

## es/app/request.html — Iniciar solicitud
**Borrador — pendiente de revisión por un hablante nativo.**
- Pedir cotizaciones · Demo
- Cuéntale a Hava qué necesitas.
- Estos pasos son una demostración. No se envían mensajes ni fotos.
- Solo prototipo: los negocios no reciben solicitudes y no se envían SMS. No incluyas datos personales.
- Primero, tu permiso.
- Ask Hava utiliza mensajes automatizados para avisarte sobre tu solicitud.
- Hi, this is Ask Hava. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: askhava.com/terms. Reply OK to start.
- Aviso legal en inglés — pendiente de revisión.
- Condiciones
- ·
- Privacidad
- Aceptar y empezar (demostración)
- ¿Qué necesitas?
- Tipo de trabajo
- Elige una categoría
- Plumbing
- AC and heating
- Electrical
- Handyman
- Appliance repair
- Garage doors
- Roofing
- Pool cleaning (weekly)
- Pool repair
- Water softeners
- Landscape maintenance
- Pressure washing
- House cleaning
- Vacation-rental turnover cleaning
- Window cleaning
- Trash-can cleaning
- Dog waste pickup
- Pest control
- Home watch
- Junk removal
- Painting
- Solar screens
- Car detailing
- Boat detailing
- Boat repair
- Local movers
- Describe el problema *
- Por ejemplo: El grifo pierde agua debajo del fregadero.
- Puede ser una emergencia
- Olor a gas o a huevo podrido: sal del lugar, no uses interruptores y llama al 911 o a la compañía de gas. Si hay chispas, humo o fuego, sal y llama al 911. Si hay inundación, ponte a salvo y pide ayuda.
- ¿Estás a salvo ahora?
- Sí
- No
- Después de ponerte a salvo, ¿qué prefieres?
- Urgente / fuera de horario (suele costar más)
- Horario normal
- Continuar tras comprobar la seguridad
- Continuar
- Atrás
- Algunos detalles.
- Código postal *
- ¿Para cuándo?
- Lo antes posible
- Esta semana
- Flexible
- ¿Eres propietario o inquilino?
- Soy propietario
- Soy inquilino
- No estaré presente
- Idioma preferido
- English
- Español
- ¿Quién puede recibir al negocio?
- Foto opcional del trabajo
- Demo: solo se muestra el nombre del archivo. No se sube.
- Enviar solicitud de ejemplo
- Atrás
- Ejemplo completado
- Los negocios locales tienen tu solicitud.
- En el servicio real te avisaremos por SMS cuando llegue la primera cotización. En esta demostración no se envió nada.
- Ver cotizaciones de ejemplo

## es/contact.html — Contacto
**Borrador — pendiente de revisión por un hablante nativo.**
- Contacto
- ¿Necesitas ayuda?
- Comunícate con Ask Hava para hacer preguntas, compartir comentarios o reportar problemas de accesibilidad.
- Ponte en contacto
- help@askhava.com
- (928) 555-0100
- Número de ejemplo — inactivo
- En este prototipo no funcionan el formulario ni los mensajes SMS. Nada de lo que escribas se enviará.
- Enviar un mensaje
- Nombre
- Email
- Mensaje
- Probar formulario (ejemplo)

## es/help.html — Ayuda y preguntas
**Borrador — pendiente de revisión por un hablante nativo.**
- Ayuda y preguntas
- Respuestas claras y sencillas.
- Preguntas frecuentes sobre cotizaciones, privacidad y Ask Hava.
- Para propietarios
- ¿Pedir cotizaciones es gratis?
- Sí. Los propietarios no pagan por pedir ni comparar cotizaciones. El negocio elegido paga si confirma.
- ¿Los negocios ven mi número?
- No. Ask Hava mantiene privado tu número y usa un número intermediario para las llamadas.
- ¿Cómo se ordenan las cotizaciones?
- Según llegan. Los negocios no pueden comprar una posición mejor.
- ¿Ask Hava recomienda negocios?
- No. Tú eliges. Consulta las reseñas y las licencias correspondientes por tu cuenta.
- ¿Cuándo reciben mi dirección?
- Solo el negocio que elijas, después de que confirmes con un código y el negocio acepte.
- ¿Cómo dejo de recibir mensajes?
- Responde STOP para dejar de recibir mensajes o HELP para pedir ayuda. No se envían mensajes promocionales.
- ¿Qué pasa si huelo gas o veo humo?
- Aléjate de la zona peligrosa y llama al 911 o a la compañía correspondiente. No uses este servicio como respuesta de emergencia.
- ¿Necesitas más ayuda?
- Contáctanos
- .

## es/how-it-works.html — Cómo funciona
**Borrador — pendiente de revisión por un hablante nativo.**
- Cómo funciona
- Tú preguntas. Los negocios cotizan. Tú eliges.
- Ask Hava reúne opciones para que decidas sin tener que hacer tantas llamadas.
- 01
- Describe el trabajo
- Envía un mensaje o usa el chat. Describe el trabajo, tu código postal y cuándo lo necesitas. La foto es opcional.
- 02
- Los negocios locales responden
- Los negocios participantes reciben una descripción anónima del trabajo. No reciben tu nombre, dirección ni número.
- 03
- Revisa todas las cotizaciones
- Lee precios, detalles, fechas, enlaces a reseñas y estado de licencia cuando corresponda. Se muestran en orden de llegada.
- 04
- Elige y confirma
- Verifica tu elección con un código por mensaje. El negocio elegido confirma y luego recibe los datos necesarios para coordinar el trabajo.
- ¿Qué queda privado?
- Antes de elegir, los negocios solo ven la descripción del trabajo. Después de verificar tu elección y de que el negocio confirme, solo ese negocio recibe tu nombre, dirección y fotos necesarias. Tu número de teléfono nunca se comparte.
- Qué no hace Hava
- Ask Hava no realiza trabajos, no recomienda un negocio por encima de otro, no garantiza la calidad ni vende posiciones en las cotizaciones. Los negocios son independientes.
- Probar la solicitud de ejemplo

## es/index.html — ¿Necesitas algo? Pregúntale a Hava.
**Borrador — pendiente de revisión por un hablante nativo.**
- Tu conserje local en Lake Havasu
- ¿Necesitas arreglar algo?
- Pregúntale a Hava.
- Cuéntale a Hava qué necesitas. Los negocios locales envían cotizaciones. Compáralas en una página privada y elige. Es gratis para ti y tu número sigue siendo privado.
- Empezar en línea
- Cómo funciona
- Escribe al
- (928) 555-0100
- ·
- Número de ejemplo — SMS no activo
- Ask Hava
- Ejemplo ilustrativo
- «Mi grifo tiene una fuga. ¿Puedes conseguirme cotizaciones?»
- «Puedo reunir cotizaciones locales. Tú las comparas y eliges.»
- Negocios locales · Tú eliges · Sin clasificaciones pagadas
- Gratis para propietarios
- Una forma más sencilla
- Una solicitud. Todas las cotizaciones juntas.
- 01
- Cuéntale a Hava
- Escribe lo que necesitas. Si quieres, agrega una foto.
- 02
- Recibe cotizaciones
- Los negocios locales que ofrecen el servicio pueden responder.
- 03
- Compara con calma
- Consulta cada cotización en orden de llegada, con las propias palabras del negocio.
- 04
- Tú decides
- Solo el negocio que elijas podrá coordinar el trabajo.
- Para la vida en Havasu
- ¿En qué necesitas ayuda?
- Desde el aire acondicionado hasta la limpieza de piscinas, solo describe lo que necesitas.
- Plomería
- ↗
- Aire acondicionado
- ↗
- Limpieza de piscinas
- ↗
- Limpieza de barcos
- ↗
- Limpieza de casas
- ↗
- Cuidado de viviendas
- ↗
- Ver todas las categorías →
- Compara sin presión
- Mira todos los detalles.
- Los negocios escriben sus cotizaciones. Hava las muestra en orden de llegada sin cambiar sus palabras ni vender las primeras posiciones.
- Ver cotizaciones de ejemplo
- Solo ejemplo
- Sample Plumbing Co.
- Empresa independiente, no afiliada a Ask Hava.
- FIRM PRICE
- $285
- «Incluye la instalación y una revisión de fugas».
- Pueden llegar más cotizaciones.
- La decisión siempre es tuya.
- Gratis para ti
- El negocio elegido paga una tarifa a Ask Hava. Tú nunca pagas por pedir o comparar cotizaciones.
- Tu número permanece privado
- Solo el negocio elegido recibe tu nombre y dirección después de confirmar. Las llamadas usan un número protegido.
- Sin spam ni puestos pagados
- Sin mensajes promocionales. Responde STOP cuando quieras. Ningún negocio puede pagar para aparecer primero.
- Tu conserje local de siempre
- Más que cotizaciones. Todo Havasu.
- Ask Hava también te ayuda a encontrar lugares, eventos y respuestas locales. Las cotizaciones son otra forma de ayudarte.
- Explorar directorio ↗
- Ver eventos locales ↗
- Preguntar al conserje ↗
- ¿Sabes qué necesitas? Pregúntale a Hava.
- Describe el trabajo una sola vez. Decide después de comparar.
- Pedir cotizaciones

## es/privacy.html — Privacidad
**Borrador — pendiente de revisión por un hablante nativo.**
- Información
- Política de privacidad
- Tu información es privada desde el principio.
- Borrador — pendiente de revisión por un abogado
- Número móvil y consentimiento
- We do not share or sell mobile numbers or SMS opt-in data with third parties or affiliates for marketing. Your name and address go only to the one business you choose, after you confirm, to arrange the job you asked for. Your phone number is never shared.
- Qué datos utiliza el servicio
- Una solicitud puede incluir descripción del trabajo, código postal, fechas, foto opcional, contacto en la propiedad, idioma y mensajes. Los plazos de almacenamiento y eliminación deben revisarse antes del lanzamiento.
- Quién ve qué
- Antes de la elección, los negocios reciben una descripción anónima. Tras verificar tu elección y la confirmación del negocio, ese negocio recibe nombre, dirección y fotos necesarias. El número móvil se mantiene privado.
- Solicitudes sobre datos
- Escribe a help@askhava.com sobre tus datos. Los procedimientos reales no están implementados en este prototipo.
- Reparaciones menores: una persona sin licencia de contratista solo puede considerarse para trabajos menores de $1,000 en total y que no requieren permiso.

## es/services.html — Categorías de servicios
**Borrador — pendiente de revisión por un hablante nativo.**
- Servicios
- Encuentra ayuda local para lo que necesitas.
- Elige una categoría para comenzar. La disponibilidad depende de negocios participantes; algunas categorías llegarán después.
- Reparaciones del hogar
- Plomería
- ↗
- Aire acondicionado y calefacción
- ↗
- Electricidad
- ↗
- Reparaciones menores
- ↗
- Reparación de electrodomésticos
- ↗
- Puertas de garaje
- ↗
- Techos
- ↗
- Piscina y agua
- Limpieza de piscinas (semanal)
- ↗
- Reparación de piscinas
- ↗
- Ablandadores de agua
- ↗
- Jardín y exteriores
- Mantenimiento de jardines
- ↗
- Jardines desérticos
- Próximamente
- Riego
- Próximamente
- Lavado a presión
- ↗
- Limpieza y mantenimiento
- Limpieza del hogar
- ↗
- Limpieza de alquileres vacacionales
- ↗
- Limpieza de ventanas
- ↗
- Lavado de botes de basura
- ↗
- Recolección de desechos de mascotas
- ↗
- Control de plagas
- ↗
- Vigilancia de viviendas
- ↗
- Retiro de basura y objetos
- ↗
- Interior y exterior
- Pintura
- ↗
- Mallas solares
- ↗
- Vehículos y embarcaciones
- Limpieza detallada de autos
- ↗
- Limpieza detallada de barcos
- ↗
- Reparación de barcos
- ↗
- Mudanzas
- Mudanzas locales
- ↗
- Reparaciones menores: una persona sin licencia de contratista solo puede considerarse para trabajos menores de $1,000 en total y que no requieren permiso.

## es/sms-terms.html — Condiciones de SMS
**Borrador — pendiente de revisión por un hablante nativo.**
- Información
- Condiciones de mensajes SMS
- Mensajes sobre tu solicitud de cotizaciones.
- Prototipo — sin mensajes reales
- El número que aparece es de ejemplo. No se envían SMS.
- Consentimiento de muestra
- Hi, this is Ask Hava. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: askhava.com/terms. Reply OK to start.
- Este aviso en inglés se conserva para revisión de cumplimiento. La traducción debe revisarse profesionalmente.
- Mensajes y baja
- La frecuencia depende de la solicitud. Pueden aplicarse tarifas de mensajes y datos. Responde STOP para darte de baja o HELP para ayuda. No enviamos mensajes promocionales. Para cerrar una solicitud por SMS, responde CLOSE 1042, no CANCEL.
- Contacto para SMS: (928) 555-0100 (número de ejemplo).
- help@askhava.com
- Reparaciones menores: una persona sin licencia de contratista solo puede considerarse para trabajos menores de $1,000 en total y que no requieren permiso.

## es/terms.html — Condiciones
**Borrador — pendiente de revisión por un hablante nativo.**
- Información
- Condiciones de uso
- Resumen de cómo funciona este servicio.
- Borrador — pendiente de revisión por un abogado
- Sobre el servicio
- Ask Hava reúne cotizaciones de negocios independientes para que los clientes las comparen. No es contratista ni realiza, supervisa o garantiza el trabajo.
- Cotizaciones e independencia
- Cada negocio escribe su cotización. Se muestran por orden de llegada, nunca por pago. El propietario elige; Ask Hava no recomienda proveedores.
- Seguridad y emergencias
- No uses este servicio en lugar de llamar al 911 o a tu compañía de servicios si hay peligro. Las reparaciones fuera de horario pueden costar más.
- Tu control
- Los propietarios no pagan y verifican su elección por código de teléfono. Puedes cerrar tu solicitud. Las condiciones legales definitivas requieren revisión profesional.
- Reparaciones menores: una persona sin licencia de contratista solo puede considerarse para trabajos menores de $1,000 en total y que no requieren permiso.

## help.html — Help and FAQ
- Help and FAQ
- Clear answers. No runaround.
- Common questions about quotes, privacy and how Ask Hava works.
- For homeowners
- Is it free to ask for quotes?
- Yes. Homeowners pay nothing to request or compare quotes. The business you choose pays a fee if it confirms.
- Can businesses see my phone number?
- No. Ask Hava keeps your phone number private and uses a masked number for calls.
- How are quotes ordered?
- In the order they arrive. Businesses cannot buy a better position.
- Does Ask Hava recommend a business?
- No. You make that choice. Check reviews and relevant license information yourself.
- When do businesses receive my address?
- Only the business you choose, after your phone-code confirmation and its acceptance.
- How do I stop texts?
- Reply STOP to opt out, or HELP for help. No marketing texts are sent.
- What if I smell gas or see smoke?
- Leave the danger area and call 911 or the appropriate utility. Do not rely on the quote service for an emergency response.
- Questions from businesses
- When do I pay?
- Only if a homeowner picks your quote and you confirm. Your first win is free.
- Is this an endorsement?
- No. “Quotes through Ask Hava” only means you opted in to receive requests.
- Do I pay if I cannot reach the homeowner?
- There is an automatic refund under the stated contact-attempt rules. See
- business pricing
- .
- Still need help?
- Contact us
- .

## how-it-works.html — How it works
- How it works
- You ask. Businesses quote. You choose.
- Ask Hava gathers options so you can make your own choice, without the phone tag.
- 01
- Tell us about the job
- Use a text or the online chat. Tell us what needs doing, your ZIP code and timing. A photo is optional.
- 02
- Local businesses can respond
- Businesses that opted in to this work type can receive an anonymous job card. No name, address or number is given to them.
- 03
- Review every quote
- Read prices, the business’s own wording, timing, review links and license status when it applies. Quotes appear in arrival order.
- 04
- Choose and confirm
- Verify your choice with a code sent to your phone. The chosen business confirms, then gets the details needed to arrange the job.
- What stays private?
- Before you choose, businesses only see job details without your name, address or phone number. After you enter the verification code and the business confirms, only that business gets your first name and address and any job photos needed. Your phone number is never shared.
- What Hava does not do
- Ask Hava does not perform the work, recommend one business over another, guarantee quality, or sell placement on quote pages. Businesses are independent.
- Try the request demo

## index.html — Need something done? Ask Hava.
- Your Lake Havasu concierge
- Need something done?
- Ask Hava.
- Tell Hava what you need. Local businesses send quotes. Compare them on one private page, then choose. It’s free for you, and your number stays private.
- Start online
- See how it works
- Text
- (928) 555-0100
- ·
- Demo number — texting is not live
- Ask Hava
- Illustrative demo
- “My kitchen faucet is leaking. Can you get me quotes?”
- “I can help gather local quotes. You’ll compare them and choose.”
- Local businesses · Your choice · No paid rankings
- Free for homeowners
- A simpler way to get things done
- One request. Every quote on one page.
- 01
- Tell Hava
- Text or chat about the job. Add a photo if it helps.
- 02
- Businesses quote
- Local businesses that offer the service can respond.
- 03
- Compare freely
- See each quote in arrival order, with the business’s own words.
- 04
- You choose
- Only the business you pick can arrange the job.
- Made for real life in Havasu
- What do you need help with?
- From summer AC repairs to keeping the pool clean, start with a simple description.
- Plumbing
- ↗
- AC and heating
- ↗
- Pool cleaning
- ↗
- Boat detailing
- ↗
- House cleaning
- ↗
- Home watch
- ↗
- Browse all service categories →
- Compare without pressure
- See the full picture.
- Businesses write their own quotes. Hava shows them in arrival order, without changing their words or selling the top spot.
- Explore sample quote page
- Sample only
- Sample Plumbing Co.
- Independent business, not affiliated with Ask Hava.
- FIRM PRICE
- $285
- “Includes installation labor and a leak check.”
- Additional quotes may still arrive.
- Your choice stays yours.
- Free for you
- The chosen business pays Ask Hava a fee. You never pay to request or compare quotes.
- Your number stays private
- Only your chosen business gets your first name and address after it confirms. Calls use a masked number.
- No spam or paid rankings
- No marketing texts. Reply STOP anytime. No business can pay to appear higher on a quote page.
- Still your local concierge
- More than quotes. All of Havasu.
- Ask Hava also helps you discover places, events and local answers. The quote feature is one more way to get something done.
- Explore the directory ↗
- See local events ↗
- Ask the concierge ↗
- Know what you need? Ask Hava.
- Describe the job once. Decide after you compare.
- Get quotes

## pricing.html — Business pricing
- For businesses
- One fee. Only when they choose you.
- No sign-up fee, no monthly fee and no fee for quotes that do not win.
- Chosen quote below $500
- $25
- Examples: small plumbing repair or two months of a $150 monthly pool account ($300 total).
- Chosen quote $500 to $1,500
- $75
- Examples: a $750 electrical job or a mid-size repair.
- Chosen quote above $1,500
- $199
- Examples: a $3,500 AC replacement.
- The rules
- Pay only when a homeowner picks you and you confirm.
- Your first win is free.
- Ongoing services are valued at two months of the price for fee calculation.
- Price ranges use the midpoint of the range.
- A trip-fee quote uses the set tier for the job type, established by platform policy.
- Repeat work from the same customer is free.
- No contract, sign-up fee or monthly charge.
- Refund if contact fails
- Automatic refund if you make two call attempts at least three hours apart within 72 hours and cannot reach the homeowner, or if they cancel before speaking with you.
- Example: a $150/month cleaning account counts as $300 (two months), so the fee would be $25 — except your first win is free.
- Join free

## privacy.html — Privacy
- Information
- Privacy policy
- Your information stays private by default.
- Draft — for attorney review
- Mobile number and consent data
- We do not share or sell mobile numbers or SMS opt-in data with third parties or affiliates for marketing. Your name and address go only to the one business you choose, after you confirm, to arrange the job you asked for. Your phone number is never shared.
- What the service uses
- Requests may include a job description, ZIP code, preferred time, optional photo, property contact, language preference and reply messages. Data retention and deletion periods need legal and engineering review before launch.
- Who sees what
- Before selection, opted-in businesses receive an anonymous job card. Once you verify your choice and the chosen business confirms, it receives the first name, job address and necessary job photos. Your mobile number remains private.
- Data requests
- Contact help@askhava.com about your data. Live handling procedures are not implemented in this prototype.
- Handyman services: an unlicensed handyman may only be considered for jobs under $1,000 total where no permit is required.

## services.html — Service categories
- Services
- Find local help for the jobs on your list.
- Choose a category to start a request. Availability depends on participating businesses; some categories are not yet planned for launch.
- Home repair
- Plumbing
- ↗
- AC and heating
- ↗
- Electrical
- ↗
- Handyman
- ↗
- Appliance repair
- ↗
- Garage doors
- ↗
- Roofing
- ↗
- Pool and water
- Pool cleaning (weekly)
- ↗
- Pool repair
- ↗
- Water softeners
- ↗
- Yard and outdoor
- Landscape maintenance
- ↗
- Desert landscaping
- Coming soon
- Irrigation
- Coming soon
- Pressure washing
- ↗
- Cleaning and property care
- House cleaning
- ↗
- Vacation-rental turnover cleaning
- ↗
- Window cleaning
- ↗
- Trash-can cleaning
- ↗
- Dog waste pickup
- ↗
- Pest control
- ↗
- Home watch
- ↗
- Junk removal
- ↗
- Interior and exterior
- Painting
- ↗
- Solar screens
- ↗
- Vehicles and boats
- Car detailing
- ↗
- Boat detailing
- ↗
- Boat repair
- ↗
- Moving
- Local movers
- ↗
- Handyman services: an unlicensed handyman may only be considered for jobs under $1,000 total where no permit is required.

## sms-terms.html — Text terms
- Information
- Text message terms
- Text messages about your quote request.
- Prototype — no live messages
- The number below is an example. No texts are sent from this demo.
- Sample homeowner opt-in
- Hi, this is Ask Hava. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: askhava.com/terms. Reply OK to start.
- This English disclosure is retained exactly for compliance review. Translated customer-facing language needs professional review.
- Messages and opting out
- Message frequency depends on a specific job request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. We do not send marketing texts. To close a request by SMS, reply CLOSE 1042, not CANCEL.
- Texting contact: (928) 555-0100 (sample number).
- help@askhava.com
- Handyman services: an unlicensed handyman may only be considered for jobs under $1,000 total where no permit is required.

## styleguide.html — Design system
- Design system
- Clearwater Modern, in Ask Hava colors.
- A practical guide to components and states in this plain-HTML prototype.
- Colors and contrast
- Ratios are calculated against white and ink. Not every pairing is approved. Main content uses Ink on Cream, white on Lake Teal, and Lake Dark for links.
- Token
- Hex
- Against white
- Against ink
- Ink
- #153342
- 13.26:1
- 1.00:1
- Lake teal
- #086e76
- 6.00:1
- 2.21:1
- Lake dark
- #07555d
- 8.51:1
- 1.56:1
- Cream
- #fbfaf6
- 1.04:1
- 12.69:1
- Paper
- #ffffff
- 1.00:1
- 13.26:1
- Lake light
- #e3f0ef
- 1.17:1
- 11.35:1
- Sand
- #efe8db
- 1.22:1
- 10.88:1
- Focus outline
- #9a4900
- 6.31:1
- 2.10:1
- Alert
- #8c291d
- 8.57:1
- 1.55:1
- Typography
- Beautifully simple.
- DM Serif Display for headings, DM Sans for everyday reading. Body text is 18px or larger.
- Buttons
- Primary
- Secondary
- Quiet
- Disabled
- Use Tab to inspect the focus indicator.
- Form fields
- Normal input
- Input with an error
- Please fill in this field.
- Select field
- Example option
- Disabled field
- Status chips
- Collecting quotes
- Quotes ready
- Needs confirmation
- Paused · Attention
- Alerts
- Informational notice: data is private by default.
- Success: saved in the demo.
- Important: leave the area if you smell gas.
- Quote card
- Sample business
- Sample Plumbing Co.
- Independent business, not affiliated with Ask Hava.
- FIRM PRICE
- $285
- “Labor and leak check included.”
- Choose
- Spacing and layout
- Card radius: 1.25rem · Section spacing: 5rem on desktop / 3.2rem on phones · Max content: 1160px · Buttons: at least 44px tall.

## terms.html — Terms
- Information
- Terms of use
- An outline of how this service works.
- Draft — for attorney review
- About the service
- Ask Hava gathers independent businesses’ quotes and presents them for customers to compare. It is not a contractor and does not perform, supervise or guarantee the work.
- Quotes and independence
- Quote content is supplied by each business. Quotes are shown in arrival order and never ranked by payment. Homeowners choose; Ask Hava does not endorse or recommend any provider.
- Safety and emergencies
- Do not use this service instead of calling 911 or your utility when danger is present. After-hours repairs can cost more.
- Your control
- No homeowner fee; verification by phone code before selection. You may close your request. Live terms, liability provisions and dispute terms require attorney review before launch.
- Handyman services: an unlicensed handyman may only be considered for jobs under $1,000 total where no permit is required.

## Dynamic and conditional copy

Interactive labels, announcements and validation messages are maintained in `assets/js/flows.js` and sample quotes in `assets/js/sample-data.js`. Review both files with this inventory before launch.

## Shared header and footer content

### EN
- a Ask Hava Menu ☰ Home Explore Havasu ↗ Events ↗ How it works For businesses Get quotes
- Built for Lake Havasu · Quotes are a new Ask Hava feature English / Español
- a Ask Hava Local answers. Local quotes. Your choice. Lake Havasu City, Arizona Explore Ask Hava directory ↗ Local events ↗ Get quotes Support Help and FAQ Contact Accessibility Texting terms For businesses Ask Hava for Business Pricing Join free Privacy Terms © 2026 Ask Hava · Prototype — not a live service Help: help@askhava.com · (928) 555-0100 (sample number)
### ES
- a Ask Hava Menú ☰ Inicio Explorar Havasu ↗ Eventos ↗ Cómo funciona Para negocios Pedir cotizaciones
- Para Lake Havasu · Las cotizaciones son una nueva función de Ask Hava English / Español
- a Ask Hava Respuestas locales. Cotizaciones locales. Tú decides. Lake Havasu City, Arizona Explorar Directorio Ask Hava ↗ Eventos locales ↗ Pedir cotizaciones Ayuda Ayuda y preguntas Contacto Accesibilidad Condiciones de SMS Para negocios Ask Hava for Business Precios Registro gratuito Privacidad Condiciones © 2026 Ask Hava · Prototipo — servicio no disponible (Prototype — not a live service) Ayuda: help@askhava.com · (928) 555-0100 (número de ejemplo)

## Exact source disclosures (business logic and legal copy)

### Homeowner SMS opt-in

> Hi, this is Ask Hava. We gather quotes so you don't have to make the calls. It's free for you, and the business you choose pays us a small fee. Msg frequency varies by request. Msg & data rates may apply. Reply STOP to opt out, HELP for help. Terms and privacy: askhava.com/terms. Reply OK to start.

### Business opt-in

> I agree to receive job notices and account texts from Ask Hava at this number. Msg frequency varies. Msg & data rates may apply. Reply STOP to opt out, HELP for help.

### Privacy clause

> We do not share or sell mobile numbers or SMS opt-in data with third parties or affiliates for marketing. Your name and address go only to the one business you choose, after you confirm, to arrange the job you asked for. Your phone number is never shared.

## Dynamic strings — full runtime copy source

The source is included below in full because UI strings are conditional and many are translated at runtime.

```javascript
/* Front-end demonstration only. No network requests and no actual text messages. */
(function(){
  const D=window.AskHavaSamples;
  const $=(s,p=document)=>p.querySelector(s);
  const $$=(s,p=document)=>Array.from(p.querySelectorAll(s));
  const lang=document.documentElement.lang==='es'?'es':'en';
  const tr=(en,es)=>lang==='es'?es:en;
  const announce=(message)=>{const x=$('#flow-status');if(x)x.textContent=message;};
  const show=(el)=>{if(el)el.hidden=false;}; const hide=el=>{if(el)el.hidden=true;};
  const escape=s=>String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  function initRequest(){
    const root=$('#request-app');if(!root)return;
    const panes=$$('[data-step]',root);let step=0;let danger=false;
    const progress=$('#request-progress');
    const params=new URLSearchParams(window.location.search);
    const requestedCategory=params.get('category'); const select=$('#request-category');
    if(requestedCategory&&select&&Array.from(select.options).some(o=>o.value===requestedCategory)){select.value=requestedCategory;}
    const repeat=params.get('repeat');if(repeat&&$('#repeat-note')){$('#repeat-note').hidden=false;$('#repeat-note').textContent=tr('Request a repeat visit from ','Solicita otra visita de ')+repeat+' (sample request only)';}
    function nextStep(n){panes.forEach((p,i)=>p.hidden=i!==n);step=n;progress.innerHTML=panes.map((p,i)=>`<span class="dot ${i===n?'active':''}">${i+1} ${i===n?'•':''}</span>`).join('');const hd=$('h2',panes[n]);if(hd){hd.tabIndex=-1;hd.focus();}announce(tr('Step '+(n+1)+' of '+panes.length,'Paso '+(n+1)+' de '+panes.length));}
    root.addEventListener('click',e=>{
      const next=e.target.closest('[data-next]');const prev=e.target.closest('[data-prev]');
      if(prev){nextStep(Math.max(0,step-1));return;}
      if(next){let current=panes[step];let missing=$$('[data-required]',current).find(x=>!x.value.trim());
        if(missing){missing.setAttribute('aria-invalid','true');const err=$('.error',current);if(err)err.textContent=tr('Please complete the required fields.','Completa los campos obligatorios.');missing.focus();return;}
        if(step===1){const problem=$('#request-problem').value.toLowerCase();danger=/(gas smell|rotten egg|sparks|smoke|burning|flood|olor a gas|huevo podrido|chispas|humo|inundaci[oó]n)/.test(problem);if(danger){show($('#safety-branch'));$('#safety-branch').scrollIntoView({block:'nearest'});announce(tr('Possible emergency. Read safety guidance first.','Posible emergencia. Lee el aviso de seguridad.'));return;}}
        nextStep(Math.min(panes.length-1,step+1));
      }
    });
    const safetyProceed=$('#safety-continue');if(safetyProceed)safetyProceed.addEventListener('click',()=>{
      const safe=$('input[name=safe]:checked');const urgency=$('input[name=urgent]:checked');
      if(!safe||!urgency){announce(tr('Answer the safety questions first.','Responde primero las preguntas de seguridad.'));return;}
      if(safe.value==='no'){announce(tr('Please move to safety and call 911 before continuing.','Ponte a salvo y llama al 911 antes de continuar.'));return;}
      hide($('#safety-branch'));nextStep(2);
    });
    $('#request-ownership')?.addEventListener('change',e=>{const x=$('#onsite-wrap');x.hidden=e.target.value==='owner'||!e.target.value;});
    $('#request-photo')?.addEventListener('change',e=>{const f=e.target.files?.[0];$('#photo-name').textContent=f?tr('Selected locally (not uploaded): ','Seleccionado localmente (no enviado): ')+f.name:'';});
    $('#request-submit')?.addEventListener('click',()=>{const zip=$('#request-zip');if(!/^\d{5}$/.test(zip.value)){zip.setAttribute('aria-invalid','true');zip.focus();announce(tr('Enter a five-digit ZIP code.','Escribe un código postal de cinco dígitos.'));return;}nextStep(3);});
    nextStep(0);
  }
  function modal(dialog,focusSelector){
    let last=document.activeElement;dialog.showModal();const target=focusSelector?$(focusSelector,dialog):$('button,input,textarea',dialog);target?.focus();
    dialog.addEventListener('close',()=>last?.focus(),{once:true});
  }
  function initQuotes(){
    const root=$('#quotes-app');if(!root)return;const list=$('#quotes-list');const recurring=$('#recurring-list');let selected=null;
    D.quotes.forEach((q,i)=>{
      const price=q.type==='range'?`$${q.min}–$${q.max}`:`$${q.price}${q.type==='monthly'?tr(' / month',' / mes'):''}`;
      const license=q.licenseRequired?`${escape(q.license)} ${tr('shows active on the state website as of 10/26/2026 (sample wording only; no verification was performed).','aparece activa en el sitio del estado al 26/10/2026 (texto de muestra; no se verificó).')}`:tr('No contractor license claim is made for this example.','No se declara licencia de contratista para este ejemplo.');
      const note=q.type==='trip'?`<p class="error">${tr('This is not a repair price.','Esto no es el precio de la reparación.')}</p>`:'';
      const card=document.createElement('article');card.className='quote-card';card.innerHTML=`<header><div><span class="tag">${tr('Sample business','Negocio ficticio')}</span><h3>${escape(q.name)}</h3></div><small>${tr(q.received,'Recibida '+(i+1)+'ª')} · ${tr('Arrival order','Orden de llegada')}</small></header>
      <p class="fine">${tr('Independent business, not affiliated with Ask Hava.','Empresa independiente, no afiliada a Ask Hava.')}</p>
      <div class="quote-kind">${escape(q.kind)}</div><div class="quote-price">${price}</div>${note}
      <p class="quote-words">“${escape(q.words)}”</p><dl class="quote-meta"><div><dt>${tr('Included','Incluye')}</dt><dd>${escape(q.includes)}</dd></div><div><dt>${tr('Earliest visit','Primera visita')}</dt><dd>${escape(q.arrival)}</dd></div><div><dt>${tr('Quote good until','Válida hasta')}</dt><dd>${escape(q.expires)}</dd></div><div><dt>${tr('License example','Ejemplo de licencia')}</dt><dd>${license}</dd></div></dl>
      <p class="fine"><a href="https://azroc.my.site.com/AZRoc/s/contractor-search" target="_blank" rel="noopener">${tr('Check it yourself — Arizona ROC','Compruébala en Arizona ROC')}</a> · <a href="#review-note" class="reviews-demo">${tr('Read their reviews (demo placeholder)','Ver reseñas (ejemplo)')}</a></p>
      <div class="pill-list">${q.spanish?'<span class="tag tag-soft">'+tr('We speak Spanish','Hablamos español')+'</span>':''}${q.afterHours?'<span class="tag tag-soft">'+tr('After-hours available','Horario extendido')+'</span>':''}</div>
      <div class="button-row" style="margin-top:1.3rem"><button class="btn" type="button" data-choose="${q.id}">${tr('Choose this quote','Elegir esta cotización')}</button></div>`;(q.type==='monthly'&&recurring?recurring:list).appendChild(card);
    });
    const choice=$('#choose-dialog');root.addEventListener('click',e=>{const b=e.target.closest('[data-choose]');if(b){selected=D.quotes.find(q=>q.id===b.dataset.choose);$('#chosen-business').textContent=selected.name;$('#demo-code').value='';$('#code-error').textContent='';modal(choice,'#demo-code');}});
    $('#choose-form')?.addEventListener('submit',e=>{e.preventDefault();const inp=$('#demo-code');if(inp.value.trim()!==D.demoCode){inp.setAttribute('aria-invalid','true');$('#code-error').textContent=tr('Incorrect demo code. Enter 123456.','Código de demostración incorrecto. Escribe 123456.');inp.focus();return;}
      inp.removeAttribute('aria-invalid');choice.close();hide($('#quotes-main'));show($('#quote-success'));$('#picked-name').textContent=selected.name;$('#quote-success h2').tabIndex=-1;$('#quote-success h2').focus();announce(tr('Choice confirmed in this demo. No text was sent.','Elección confirmada en esta demostración. No se envió ningún mensaje.'));
    });
    $$('.dialog-close').forEach(x=>x.addEventListener('click',()=>x.closest('dialog').close()));
    $$('[data-dialog]').forEach(b=>b.addEventListener('click',()=>modal($(b.dataset.dialog))));
    $('#close-request')?.addEventListener('click',()=>{modal($('#close-dialog'));});
    $('#confirm-close')?.addEventListener('click',()=>{$('#close-dialog').close();$('#quotes-main').innerHTML='<div class="notice">'+tr('Sample request closed. Nothing was changed on a server.','Solicitud de ejemplo cerrada. No se modificó ningún servidor.')+'</div>';announce(tr('Request closed in the demo.','Solicitud cerrada en la demostración.'));});
    $$('[data-demo-action]').forEach(b=>b.addEventListener('click',()=>{const d=b.closest('dialog');d.close();announce(tr('Demo saved locally. Nothing was sent.','Demostración guardada localmente. No se envió nada.'));}));
    $('#reviews-placeholder')?.addEventListener('click',e=>e.preventDefault());
    root.addEventListener('click',e=>{if(e.target.closest('.reviews-demo')){e.preventDefault();announce(tr('Real review links will be provided by businesses in the live service.','Los enlaces a reseñas estarán disponibles en el servicio real.'));}});
  }
  function initPortal(){
    const root=$('#portal-app');if(!root)return;
    $('#signin-form')?.addEventListener('submit',e=>{e.preventDefault();const code=$('#portal-code');if(code.value!==D.demoCode){$('#signin-error').textContent=tr('Use 123456 for this demo.','Usa 123456 en esta demostración.');code.focus();return;}hide($('#portal-signin'));show($('#portal-content'));announce(tr('Demo account opened.','Cuenta de demostración abierta.'));$('#portal-content h2').tabIndex=-1;$('#portal-content h2').focus();});
    $$('.portal-tab',root).forEach(b=>b.addEventListener('click',()=>{$$('.portal-tab',root).forEach(x=>{x.setAttribute('aria-selected',String(x===b));});$$('[data-portal-panel]',root).forEach(x=>x.hidden=x.id!==b.dataset.target);$('#'+b.dataset.target+' h3')?.focus();}));
    $$('.local-save',root).forEach(b=>b.addEventListener('click',()=>announce(tr('Saved for this demo only. No server was contacted.','Guardado solo para esta demostración. No se contactó ningún servidor.'))));
    $('#delete-demo')?.addEventListener('click',()=>{if(window.confirm(tr('Reset this demo view? No real data exists.','¿Reiniciar esta vista de demostración? No existen datos reales.'))){hide($('#portal-content'));show($('#portal-signin'));announce(tr('Demo view reset.','Vista reiniciada.'));}});
  }
  function initSignup(){
    const root=$('#signup-app');if(!root)return;let idx=0;const steps=$$('[data-signup-step]',root);
    const switchStep=n=>{idx=n;steps.forEach((s,i)=>s.hidden=i!==n);$('#signup-progress').textContent=`${tr('Step','Paso')} ${n+1} / ${steps.length}`;$('h2',steps[n])?.focus();};
    $('#lookup-license')?.addEventListener('click',()=>{const code=$('#license-number').value.trim();const status=$('#license-status');if(code==='000000'){status.className='notice notice-success';status.textContent='SAMPLE LOOKUP — R-39 (Air conditioning and refrigeration). It covers AC repair, heating and system replacement. This is fake data; no state lookup happened.';$('#license-lookup-ok').value='yes';}else{status.className='notice notice-danger';status.textContent='Demo only: enter 000000 to show the sample license lookup.';$('#license-lookup-ok').value='';}});
    $('#license-type')?.addEventListener('change',e=>{const none=e.target.value==='none';$('#license-wrap').hidden=none;$('#unlicensed-note').hidden=!none;$('#licensed-work').hidden=none;$('#unlicensed-work').hidden=!none;$$('input[name=work]',none?$('#licensed-work'):$('#unlicensed-work')).forEach(x=>x.checked=false);});
    root.addEventListener('click',e=>{if(e.target.closest('[data-signup-prev]'))switchStep(Math.max(0,idx-1));if(e.target.closest('[data-signup-next]')){
      if(idx===0){const name=$('#biz-name');const license=$('#license-type');if(!name.value.trim()){name.focus();announce('Enter your business name.');return;}if(license.value!=='none'&&$('#license-lookup-ok').value!=='yes'){announce('Use the sample license number 000000 and look it up first.');$('#license-number').focus();return;}}
      if(idx===1){const checked=$$('input[name=work]:checked');if(!checked.length){announce('Choose at least one service.');return;}if($('#license-type').value!=='none'&&checked.some(x=>x.dataset.covered==='no')){announce('That job type needs a different license. Remove it to continue.');return;}}
      if(idx===3){if($('#identity-code').value!=='123456'){announce('Enter the sample identity code 123456. No real code was sent.');$('#identity-code').focus();return;}if(!$('#business-consent').checked){announce('Texting consent is required for job notices. The box starts unchecked.');$('#business-consent').focus();return;}}
      switchStep(Math.min(steps.length-1,idx+1));
    }});
    $('#biz-identity')?.addEventListener('change',e=>{$('#identity-method-label').textContent={sms:'We will text the number on the state record (demo only).',call:'We will call the number on the state record (demo only).',email:'We will email the address on the state record (demo only).',mail:'We will mail a letter to the state-record address (demo only).'}[e.target.value];});
    switchStep(0);
  }
  function initBusiness(){
    const root=$('#business-app');if(!root)return;
    $$('.business-tab',root).forEach(b=>b.addEventListener('click',()=>{$$('.business-tab',root).forEach(x=>x.setAttribute('aria-selected',String(b===x)));$$('[data-business-panel]',root).forEach(p=>p.hidden=p.id!==b.dataset.target);}));
    const kind=$('#business-price-type');const recalc=()=>{if(!kind)return;const a=parseFloat($('#business-amount').value)||0;const b=parseFloat($('#business-max').value)||a;const middle=kind.value==='range'?(a+b)/2:kind.value==='monthly'?a*2:a;const fee=kind.value==='trip'?75:middle<500?25:middle<=1500?75:199;$('#live-tier').textContent=`$${fee} (demo estimate)`;$('#max-field').hidden=kind.value!=='range';};kind?.addEventListener('change',recalc);$('#business-amount')?.addEventListener('input',recalc);$('#business-max')?.addEventListener('input',recalc);recalc();
    $('#quote-submit')?.addEventListener('click',()=>{if(!$('#business-amount').value){$('#business-amount').focus();announce('Enter an amount.');return;}announce('Sample quote saved. No homeowner was contacted.');});
    $$('.confirm-pick',root).forEach(b=>b.addEventListener('click',()=>{b.disabled=true;b.textContent='Confirmed in demo';announce('Sample selection confirmed. In production the fee would be charged and masked calling activated.');}));
    $$('.business-save',root).forEach(b=>b.addEventListener('click',()=>announce('Business demo settings saved on this page only.')));
  }
  function initOwner(){
    const root=$('#owner-app');if(!root)return;$$('input[type=checkbox]',root).forEach(x=>x.addEventListener('change',()=>announce(`${x.dataset.label}: ${x.checked?'Paused in the demo':'Running in the demo'}. No production system affected.`)));
  }
  initRequest();initQuotes();initPortal();initSignup();initBusiness();initOwner();
})();

```

## Fictional quote card data — full source

```javascript
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

```
